import Foundation
import CryptoKit
import os

/// Service for importing vocabulary from JSON files and CSV files
final class VocabImportService {
    static let shared = VocabImportService()
    private let logger = Logger(subsystem: "com.wordgame.vocab", category: "VocabImportService")

    private init() {}

    private func needsPresetReimport(existingBook: WordBook, expectedWordCount: Int, bundleURL: URL) throws -> Bool {
        // Always reimport if word count changed (e.g. new version of the vocabulary)
        if existingBook.wordCount != expectedWordCount {
            return true
        }

        // For stable vocabularies (CET-4) that haven't changed count: use SHA256 content hash.
        // If the JSON file content changed (new translations, corrected data, etc.),
        // the hash will differ and trigger a clean reimport.
        let data = try Data(contentsOf: bundleURL)
        let hash = SHA256.hash(data: data)
        let hashString = hash.compactMap { String(format: "%02x", $0) }.joined()
        let storedHash = UserDefaults.standard.string(forKey: "vocab_hash_\(existingBook.id)")

        if storedHash != hashString {
            // Content changed — mark updated hash and reimport
            UserDefaults.standard.set(hashString, forKey: "vocab_hash_\(existingBook.id)")
            return true
        }

        return false
    }

    // MARK: - JSON Import
    /// Import vocabulary from a JSON file in the app bundle.
    ///
    /// Import semantics by preset type:
    /// - **CET-4**: Stable vocabulary. If already imported → skip (no update).
    /// - **high_school_3500**: Versioned. If already imported with same word count → skip;
    ///   if word count differs (new version detected) → replace with fresh import.
    func importPresetVocabulary(_ preset: PresetVocabulary) async throws {
        // Load vocabulary data from bundle (support both root and Vocabularies/ subdirectory).
        let url =
            Bundle.main.url(forResource: preset.rawValue, withExtension: "json", subdirectory: "Vocabularies") ??
            Bundle.main.url(forResource: preset.rawValue, withExtension: "json")

        guard let url else {
            throw VocabImportError.fileNotFound
        }

        let data = try Data(contentsOf: url)
        let vocabulary = try JSONDecoder().decode(VocabularyFile.self, from: data)
        let expectedWordCount = vocabulary.words.count

        // Check if this preset already exists in the database.
        if let existingBook = try DatabaseService.shared.fetchPresetVocabulary(preset) {
            if try needsPresetReimport(existingBook: existingBook, expectedWordCount: expectedWordCount, bundleURL: url) {
                logger.info("Preset '\(preset.displayName)' is outdated or incomplete, re-importing...")
                try DatabaseService.shared.deleteWordBook(byId: existingBook.id)
            } else {
                logger.info("Preset '\(preset.displayName)' already up-to-date (\(expectedWordCount) words), skipping.")
                return
            }
        }

        // Create the word book and bulk-insert all words.
        // Use preset's rawValue as the ID so fetchPresetVocabulary can
        // match by ID regardless of whether the user renamed the book.
        let book = WordBook(
            id: preset.rawValue,
            name: preset.displayName,
            description: preset.description,
            wordCount: expectedWordCount,
            isPreset: true
        )
        try DatabaseService.shared.createWordBook(book)

        let words = vocabulary.words.map { vocabWord in
            Word(
                bookId: book.id,
                word: vocabWord.word,
                phonetic: vocabWord.phonetic,
                meaning: vocabWord.meaning,
                sentence: vocabWord.sentence,
                sentenceTranslation: vocabWord.sentenceTranslation
            )
        }

        // Use an atomic transaction so partial failures don't leave orphaned book records.
        do {
            try DatabaseService.shared.createWordBookAndWordsAtomically(book: book, words: words)
        } catch {
            // If the atomic insert failed, attempt to clean up the half-created book.
            try? DatabaseService.shared.deleteWordBook(byId: book.id)
            throw error
        }
    }

    /// Initialize all preset vocabularies
    func initializePresetVocabularies() async {
        for preset in PresetVocabulary.allCases {
            do {
                try await importPresetVocabulary(preset)
                logger.info("Imported vocabulary: \(preset.displayName)")
            } catch {
                logger.error("Failed to import \(preset.displayName): \(error.localizedDescription)")
            }
        }
    }

    // MARK: - CSV Import
    /// Import vocabulary from a CSV file
    func importCSV(from url: URL, bookName: String, bookDescription: String?) async throws -> WordBook {
        let content = try String(contentsOf: url, encoding: .utf8)
        return try await importCSV(content: content, bookName: bookName, bookDescription: bookDescription)
    }

    /// Import vocabulary from CSV content string
    func importCSV(content: String, bookName: String, bookDescription: String?) async throws -> WordBook {
        var lines = content.components(separatedBy: .newlines)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .filter { !$0.isEmpty }

        // Remove header if present
        if lines.first?.lowercased().hasPrefix("word") == true {
            lines.removeFirst()
        }

        // Parse words
        var words: [Word] = []
        var errors: [String] = []

        for (index, line) in lines.enumerated() {
            let parts = parseCSVLine(line)

            guard parts.count >= 2 else {
                errors.append("Line \(index + 1): Invalid format, expected at least word and meaning")
                continue
            }

            let word = Word(
                bookId: "",  // Will be set after book creation
                word: parts[0].trimmingCharacters(in: .whitespaces),
                phonetic: parts.count > 1 ? parts[1].trimmingCharacters(in: .whitespaces) : nil,
                meaning: parts.count > 2 ? parts[2].trimmingCharacters(in: .whitespaces) : parts[1].trimmingCharacters(in: .whitespaces),
                sentence: parts.count > 3 ? parts[3].trimmingCharacters(in: .whitespaces) : nil,
                sentenceTranslation: parts.count > 4 ? parts[4].trimmingCharacters(in: .whitespaces) : nil
            )

            if word.word.isEmpty {
                errors.append("Line \(index + 1): Empty word")
                continue
            }

            words.append(word)
        }

        if !errors.isEmpty {
            logger.warning("CSV Import warnings: \(errors.prefix(5).joined(separator: "; "))")
        }

        // Create word book
        let book = WordBook(
            name: bookName,
            description: bookDescription,
            wordCount: words.count,
            isPreset: false
        )

        try DatabaseService.shared.createWordBook(book)

        // Update word book IDs and save
        let wordsWithBookId = words.map { word in
            Word(
                id: word.id,
                bookId: book.id,
                word: word.word,
                phonetic: word.phonetic,
                meaning: word.meaning,
                sentence: word.sentence,
                sentenceTranslation: word.sentenceTranslation
            )
        }

        try DatabaseService.shared.createWords(wordsWithBookId)

        return book
    }

    /// Parse a CSV line properly handling quoted values and escaped quotes ("") within fields.
    private func parseCSVLine(_ line: String) -> [String] {
        var result: [String] = []
        var current = ""
        var inQuotes = false
        let chars = Array(line)
        var i = 0
        while i < chars.count {
            let char = chars[i]
            if char == "\"" {
                if inQuotes {
                    // Check for escaped double quote: "" inside quoted field
                    if i + 1 < chars.count && chars[i + 1] == "\"" {
                        current.append("\"")
                        i += 1
                    } else {
                        inQuotes = false
                    }
                } else {
                    inQuotes = true
                }
            } else if char == "," && !inQuotes {
                result.append(current)
                current = ""
            } else {
                current.append(char)
            }
            i += 1
        }
        result.append(current)
        return result
    }

    // MARK: - Manual Word Addition
    /// Add a single word to an existing book
    func addWord(to bookId: String, word: String, phonetic: String?, meaning: String, sentence: String?) throws {
        let newWord = Word(
            bookId: bookId,
            word: word,
            phonetic: phonetic,
            meaning: meaning,
            sentence: sentence
        )

        try DatabaseService.shared.createWord(newWord)
    }
}

// MARK: - Supporting Types
struct VocabularyFile: Codable {
    let name: String
    let description: String?
    let words: [VocabularyWord]
}

struct VocabularyWord: Codable {
    let word: String
    let phonetic: String?
    let meaning: String
    let sentence: String?
    let sentenceTranslation: String?
}

enum VocabImportError: Error, LocalizedError {
    case fileNotFound
    case invalidFormat
    case importFailed(String)

    var errorDescription: String? {
        switch self {
        case .fileNotFound:
            return "词汇文件未找到"
        case .invalidFormat:
            return "文件格式无效"
        case .importFailed(let reason):
            return "导入失败: \(reason)"
        }
    }
}
