import SwiftUI

// MARK: - My Page View (我的)
struct MyPageView: View {
    @StateObject private var viewModel = MyPageViewModel()

    var body: some View {
        ScrollView {
            VStack(spacing: 20) {
                // ── Top 20%: User Profile ─────────────────────────────────
                userProfileSection

                // ── Bottom 60%: Learning Records ─────────────────────────
                learningRecordsSection
            }
            .padding()
        }
        .background(Color.backgroundMain)
        .navigationTitle("我的")
        .onAppear { viewModel.load() }
    }

    // MARK: - User Profile Section
    private var userProfileSection: some View {
        HStack(spacing: 16) {
            // Avatar
            Circle()
                .fill(Color.primaryBlue.gradient)
                .frame(width: 72, height: 72)
                .overlay {
                    Text(viewModel.avatarInitial)
                        .font(.system(size: 28, weight: .bold))
                        .foregroundColor(.white)
                }

            VStack(alignment: .leading, spacing: 4) {
                Text(viewModel.userName)
                    .font(DesignFont.title2)
                    .foregroundColor(.primary)

                Text("每日坚持，积少成多")
                    .font(DesignFont.subheadline)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(20)
        .frame(height: 120)
        .background(Color.cardBackground)
        .cornerRadius(16)
        .shadow(color: .black.opacity(0.05), radius: 8, x: 0, y: 4)
    }

    // MARK: - Learning Records Section
    private var learningRecordsSection: some View {
        VStack(alignment: .leading, spacing: 16) {
            // Section header
            HStack {
                Text("学习记录")
                    .font(DesignFont.headline)
                Spacer()
            }

            // Stats row: words learned + days learned
            HStack(spacing: 12) {
                StatCard(
                    icon: "text.book.closed.fill",
                    iconColor: .primaryBlue,
                    title: "已学单词",
                    value: "\(viewModel.totalWordsLearned)"
                )

                StatCard(
                    icon: "calendar",
                    iconColor: .successGreen,
                    title: "累计天数",
                    value: "\(viewModel.totalLearningDays)"
                )
            }

            // Calendar
            calendarSection
        }
    }

    // MARK: - Calendar Section
    private var calendarSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Month navigation header
            HStack {
                Button {
                    viewModel.changeMonth(by: -1)
                } label: {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundColor(.primaryBlue)
                        .frame(width: 28, height: 28)
                        .background(Color.primaryBlue.opacity(0.1))
                        .cornerRadius(8)
                }
                .buttonStyle(.plain)

                Spacer()

                Text(viewModel.monthYearLabel)
                    .font(DesignFont.headline)
                    .foregroundColor(.primary)

                Spacer()

                Button {
                    viewModel.changeMonth(by: 1)
                } label: {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundColor(.primaryBlue)
                        .frame(width: 28, height: 28)
                        .background(Color.primaryBlue.opacity(0.1))
                        .cornerRadius(8)
                }
                .buttonStyle(.plain)
                .disabled(viewModel.isCurrentMonth)
            }

            // Weekday labels
            HStack(spacing: 0) {
                ForEach(["日", "一", "二", "三", "四", "五", "六"], id: \.self) { day in
                    Text(day)
                        .font(DesignFont.caption)
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity)
                }
            }

            // Calendar grid
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 0), count: 7), spacing: 8) {
                ForEach(viewModel.calendarDays, id: \.self) { day in
                    calendarDayCell(day)
                }
            }
        }
        .padding(16)
        .background(Color.cardBackground)
        .cornerRadius(16)
        .shadow(color: .black.opacity(0.05), radius: 8, x: 0, y: 4)
    }

    // MARK: - Calendar Day Cell
    private func calendarDayCell(_ day: CalendarDay) -> some View {
        VStack(spacing: 2) {
            // Day number
            Text(day.dayNumber)
                .font(DesignFont.caption)
                .fontWeight(day.isToday ? .bold : .regular)
                .foregroundColor(day.isToday ? .white : (day.isCurrentMonth ? .primary : .secondary))
                .frame(width: 28, height: 28)
                .background(
                    Circle()
                        .fill(day.isToday ? Color.primaryBlue : Color.clear)
                )

            // Word count badge
            if day.wordCount > 0 {
                Text("\(day.wordCount)")
                    .font(.system(size: 9, weight: .medium))
                    .foregroundColor(.white)
                    .frame(width: 16, height: 14)
                    .background(
                        RoundedRectangle(cornerRadius: 4)
                            .fill(Color.successGreen)
                    )
            } else {
                Text("")
                    .font(.system(size: 9))
                    .frame(width: 16, height: 14)
            }
        }
        .frame(height: 50)
        .opacity(day.isCurrentMonth ? 1 : 0.35)
    }
}

// MARK: - Stat Card
struct StatCard: View {
    let icon: String
    let iconColor: Color
    let title: String
    let value: String

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 20))
                .foregroundColor(iconColor)
                .frame(width: 40, height: 40)
                .background(iconColor.opacity(0.12))
                .cornerRadius(10)

            VStack(alignment: .leading, spacing: 2) {
                Text(value)
                    .font(DesignFont.title2)
                    .foregroundColor(.primary)
                Text(title)
                    .font(DesignFont.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()
        }
        .padding(14)
        .background(Color.cardBackground)
        .cornerRadius(14)
        .shadow(color: .black.opacity(0.04), radius: 6, x: 0, y: 3)
    }
}

// MARK: - Calendar Day Model
struct CalendarDay: Hashable {
    let dayNumber: String
    let isCurrentMonth: Bool
    let isToday: Bool
    let wordCount: Int
    let date: Date

    static func == (lhs: CalendarDay, rhs: CalendarDay) -> Bool {
        lhs.date == rhs.date
    }

    func hash(into hasher: inout Hasher) {
        hasher.combine(date)
    }
}

// MARK: - ViewModel
@MainActor
class MyPageViewModel: ObservableObject {
    // User info (placeholder — no login system yet)
    var userName: String {
        get { UserDefaults.standard.string(forKey: "userName") ?? "Guest" }
        set { UserDefaults.standard.set(newValue, forKey: "userName") }
    }

    // Stats
    @Published var totalWordsLearned: Int = 0
    @Published var totalLearningDays: Int = 0

    // Calendar
    @Published var currentMonth: Date = Date()
    @Published var calendarDays: [CalendarDay] = []

    private let calendar = Calendar.current
    private let dateFormatter: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "yyyy年 M月"
        return f
    }()

    var monthYearLabel: String { dateFormatter.string(from: currentMonth) }

    var isCurrentMonth: Bool {
        calendar.isDate(currentMonth, equalTo: Date(), toGranularity: .month)
    }

    var avatarInitial: String {
        let name = userName.isEmpty ? "G" : String(userName.prefix(1)).uppercased()
        return name
    }

    var userNameDisplay: String { userName.isEmpty ? "Guest" : userName }

    func load() {
        fetchStats()
        buildCalendar()
    }

    func changeMonth(by value: Int) {
        guard let newDate = calendar.date(byAdding: .month, value: value, to: currentMonth) else { return }
        // Don't go into the future
        if newDate > Date() { return }
        currentMonth = newDate
        buildCalendar()
    }

    private func fetchStats() {
        do {
            // Use SQL aggregation instead of loading all records into memory
            totalWordsLearned = try DatabaseService.shared.fetchUniqueWordsLearnedCount()
            totalLearningDays = try DatabaseService.shared.fetchUniqueLearningDaysCount()
        } catch {
            totalWordsLearned = 0
            totalLearningDays = 0
        }
    }

    private func buildCalendar() {
        let today = calendar.startOfDay(for: Date())

        guard let monthInterval = calendar.dateInterval(of: .month, for: currentMonth),
              let monthFirstWeek = calendar.dateInterval(of: .weekOfMonth, for: monthInterval.start)
        else { return }

        // Fetch records for this visible range (may span two months)
        let rangeStart = monthFirstWeek.start
        let rangeEnd = calendar.date(byAdding: .month, value: 2, to: monthInterval.start)!

        // Use SQL aggregation instead of loading all records
        var recordsByDay: [Date: Int] = [:]
        do {
            recordsByDay = try DatabaseService.shared.fetchLearningRecordsCountByDay(startDate: rangeStart, endDate: rangeEnd)
        } catch { /* ignore */ }

        var days: [CalendarDay] = []

        // Weeks covering this month
        var weekStart = monthFirstWeek.start
        while weekStart < monthInterval.end || calendar.component(.weekday, from: weekStart) != calendar.firstWeekday {
            for weekday in 0..<7 {
                guard let dayDate = calendar.date(byAdding: .day, value: weekday, to: weekStart) else { continue }
                let dayOfMonth = calendar.component(.day, from: dayDate)
                let isCurrentMonth = calendar.isDate(dayDate, equalTo: currentMonth, toGranularity: .month)
                let isToday = calendar.isDate(dayDate, inSameDayAs: today)

                days.append(CalendarDay(
                    dayNumber: "\(dayOfMonth)",
                    isCurrentMonth: isCurrentMonth,
                    isToday: isToday,
                    wordCount: isCurrentMonth ? (recordsByDay[calendar.startOfDay(for: dayDate)] ?? 0) : 0,
                    date: dayDate
                ))
            }
            guard let nextWeek = calendar.date(byAdding: .weekOfMonth, value: 1, to: weekStart) else { break }
            weekStart = nextWeek
            // Stop after covering the full month
            if weekStart > monthInterval.end && !calendar.isDate(weekStart, equalTo: monthInterval.end, toGranularity: .month) {
                break
            }
        }

        calendarDays = days
    }
}

// MARK: - User Name Edit View (accessible from MyPage)
struct UserNameEditView: View {
    @Environment(\.dismiss) private var dismiss
    @AppStorage("userName") private var userName: String = ""
    @State private var draftName: String = ""

    var body: some View {
        VStack(spacing: 24) {
            Text("修改昵称")
                .font(DesignFont.title3)

            TextField("输入昵称", text: $draftName)
                .textFieldStyle(.roundedBorder)
                .font(DesignFont.body)

            HStack(spacing: 16) {
                Button("取消") { dismiss() }
                    .buttonStyle(.bordered)
                Button("保存") {
                    userName = draftName
                    dismiss()
                }
                .buttonStyle(.borderedProminent)
                .disabled(draftName.trimmingCharacters(in: .whitespaces).isEmpty)
            }
        }
        .padding(32)
        .frame(width: 320)
        .onAppear { draftName = userName }
    }
}

#Preview {
    NavigationStack {
        MyPageView()
    }
    .environmentObject(DatabaseService.shared)
}
