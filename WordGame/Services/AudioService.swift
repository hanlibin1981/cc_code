import Foundation
import AVFoundation
import os

/// Service for text-to-speech audio playback
final class AudioService: ObservableObject {
    static let shared = AudioService()

    private let logger = Logger(subsystem: "com.wordgame.audio", category: "AudioService")

    @Published var isPlaying = false
    @Published var lastError: String?

    private var audioPlayer: AVAudioPlayer?
    /// Currently running say process (for interruption)
    private var sayProcess: Process?
    /// Single callback for speech finish (simplified since stop() ensures single speech at a time)
    private var speechDidFinish: (() -> Void)?

    private let synthesizer = AVSpeechSynthesizer()
    private let delegateHandler = SpeechDelegateHandler()

    private init() {
        synthesizer.delegate = delegateHandler
        delegateHandler.onSpeechFinish = { [weak self] in
            DispatchQueue.main.async {
                self?.speechDidFinish?()
                self?.speechDidFinish = nil
                self?.isPlaying = false
            }
        }
        setupAudioSession()
    }

    private func setupAudioSession() {
        #if os(macOS)
        // macOS doesn't require explicit audio session setup
        #else
        do {
            try AVAudioSession.sharedInstance().setCategory(.playback, mode: .default)
            try AVAudioSession.sharedInstance().setActive(true)
        } catch {
            logger.error("Audio session setup failed: \(error.localizedDescription)")
        }
        #endif
    }

    // MARK: - Sound Enabled Check
    private var isSoundEnabled: Bool {
        if UserDefaults.standard.object(forKey: "soundEnabled") == nil {
            return true
        }
        return UserDefaults.standard.bool(forKey: "soundEnabled")
    }

    private var preferredVoice: String {
        UserDefaults.standard.string(forKey: "ttsVoice") ?? "Alex"
    }

    // MARK: - Text-to-Speech
    /// Speak a word using system TTS
    func speak(_ text: String, language: String = "en-US") {
        guard isSoundEnabled else { return }

        #if os(macOS)
        speakWithSay(text)
        #else
        stop()

        let utterance = AVSpeechUtterance(string: text)
        utterance.voice = AVSpeechSynthesisVoice(language: language)
        utterance.rate = AVSpeechUtteranceDefaultSpeechRate * 0.8
        utterance.pitchMultiplier = 1.0
        utterance.volume = 1.0

        isPlaying = true
        synthesizer.speak(utterance)
        #endif
    }

    /// Speak a word using the `say` command (macOS native)
    func speakWithSay(_ text: String, voice: String? = nil) {
        guard isSoundEnabled else { return }
        stop()

        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/say")
        task.arguments = ["-v", voice ?? preferredVoice, text]
        sayProcess = task
        isPlaying = true

        DispatchQueue.global().async { [weak self] in
            do {
                try task.run()
                task.waitUntilExit()
                DispatchQueue.main.async {
                    // Only clear if this is still the same process
                    if self?.sayProcess === task {
                        self?.sayProcess = nil
                        self?.isPlaying = false
                    }
                }
            } catch {
                DispatchQueue.main.async {
                    if self?.sayProcess === task {
                        self?.sayProcess = nil
                        self?.isPlaying = false
                    }
                    self?.lastError = error.localizedDescription
                    self?.logger.error("say command failed: \(error.localizedDescription)")
                }
            }
        }
    }

    /// Stop any currently playing audio
    func stop() {
        if synthesizer.isSpeaking {
            synthesizer.stopSpeaking(at: .immediate)
        }
        audioPlayer?.stop()
        audioPlayer = nil
        // Terminate any running say process
        if let process = sayProcess, process.isRunning {
            process.terminate()
        }
        sayProcess = nil
        speechDidFinish = nil
        isPlaying = false
    }

    // MARK: - Word Audio Playback

    /// Play audio for a word using hybrid strategy: URL audio first, then TTS fallback
    func playWordAudio(word: Word, onFinish: (() -> Void)? = nil) {
        guard isSoundEnabled else {
            onFinish?()
            return
        }
        stop()

        // If URL exists, try to play it first
        if let urlString = word.audioUrl, let url = URL(string: urlString) {
            isPlaying = true
            playFromURL(url) { [weak self] success in
                DispatchQueue.main.async {
                    self?.isPlaying = false
                    if !success {
                        // Fallback to TTS on URL failure
                        self?.playTTSFallback(word: word, onFinish: onFinish)
                    } else {
                        onFinish?()
                    }
                }
            }
        } else {
            // No URL, use TTS directly
            playTTSFallback(word: word, onFinish: onFinish)
        }
    }

    /// Play word via TTS and fire onFinish when done
    private func playTTSFallback(word: Word, onFinish: (() -> Void)? = nil) {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/say")
        task.arguments = ["-v", preferredVoice, word.word]

        isPlaying = true

        DispatchQueue.global().async { [weak self] in
            do {
                try task.run()
                task.waitUntilExit()
                DispatchQueue.main.async {
                    self?.isPlaying = false
                    onFinish?()
                }
            } catch {
                DispatchQueue.main.async {
                    self?.isPlaying = false
                    self?.lastError = error.localizedDescription
                    self?.logger.error("TTS fallback failed: \(error.localizedDescription)")
                    onFinish?()
                }
            }
        }
    }

    /// Play audio from a URL using AVAudioPlayer
    /// Creates a fresh delegate handler per call to avoid singleton callback overwrite.
    private func playFromURL(_ url: URL, onComplete: @escaping (Bool) -> Void) {
        DispatchQueue.global().async { [weak self] in
            guard let self = self else { return }
            do {
                let player = try AVAudioPlayer(contentsOf: url)
                let handler = URLPlaybackHandler(player: player, onComplete: onComplete)
                player.delegate = handler
                // Keep handler alive by storing it; clean up when playback ends
                self.audioPlayer = player
                player.prepareToPlay()
                player.play()
            } catch {
                DispatchQueue.main.async {
                    self.lastError = "Failed to play audio from URL: \(error.localizedDescription)"
                    self.logger.error("URL playback failed: \(error.localizedDescription)")
                    onComplete(false)
                }
            }
        }
    }
}

// MARK: - URL Playback Handler (per-call, not singleton)
/// Fresh delegate instance per playFromURL call — no longer shares state across calls.
private class URLPlaybackHandler: NSObject, AVAudioPlayerDelegate {
    private let player: AVAudioPlayer
    private let onComplete: (Bool) -> Void

    init(player: AVAudioPlayer, onComplete: @escaping (Bool) -> Void) {
        self.player = player
        self.onComplete = onComplete
    }

    func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        onComplete(flag)
    }

    func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: Error?) {
        Logger(subsystem: "com.wordgame.audio", category: "AudioPlayerDelegate")
            .error("Decode error: \(error?.localizedDescription ?? "unknown")")
        onComplete(false)
    }
}

// MARK: - Speech Delegate Handler
/// Handles speech synthesizer delegate callbacks with a single callback (since stop() ensures single speech at a time).
private class SpeechDelegateHandler: NSObject, AVSpeechSynthesizerDelegate {
    var onSpeechFinish: (() -> Void)?

    func speechSynthesizer(_ synthesizer: AVSpeechSynthesizer, didFinish utterance: AVSpeechUtterance) {
        onSpeechFinish?()
    }
}
