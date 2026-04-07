import AppKit
import AVFoundation
import Carbon.HIToolbox
import Foundation
import SwiftUI

private let defaultToggleShortcutLabel = "Control + Option + Space"
private let hotKeySignature = OSType(0x57535052)

@main
struct WisprMacApp: App {
    @NSApplicationDelegateAdaptor(WisprAppDelegate.self) private var appDelegate
    @StateObject private var menuModel = MenuModel()
    @StateObject private var settingsModel = SettingsModel()

    var body: some Scene {
        MenuBarExtra("Wispr", systemImage: "mic.fill") {
            MenuContentView(menuModel: menuModel, settingsModel: settingsModel)
                .onAppear {
                    menuModel.start()
                }
        }
        .menuBarExtraStyle(.window)

        Window("Wispr Settings", id: "wispr-settings") {
            SettingsView(model: settingsModel)
                .frame(minWidth: 680, minHeight: 700)
                .onAppear {
                    focusSettingsWindow()
                    settingsModel.reload()
                }
        }
        .defaultSize(width: 760, height: 760)
    }
}

final class WisprAppDelegate: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApplication.shared.setActivationPolicy(.accessory)
        GlobalHotKeyManager.shared.registerDefaultToggle {
            WisprRuntime.shared.toggleFromHotKey()
        }
        WisprRuntime.shared.requestMicrophoneAccessIfNeeded()
        WisprRuntime.shared.ensureDaemonRunningAsync()
    }
}

struct MenuContentView: View {
    @ObservedObject var menuModel: MenuModel
    @ObservedObject var settingsModel: SettingsModel
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Wispr Dictation")
                .font(.headline)

            Text(menuModel.lastStatus)
                .font(.footnote)
                .foregroundStyle(.secondary)
                .lineLimit(6)

            if !menuModel.lastCommandMessage.isEmpty {
                Text(menuModel.lastCommandMessage)
                    .font(.footnote)
                    .foregroundStyle(menuModel.lastCommandWasError ? .red : .secondary)
                    .lineLimit(6)
                    .textSelection(.enabled)
            }

            Divider()

            Button("Toggle Dictation") {
                menuModel.runControl("toggle")
            }
            Button("Start Dictation") {
                menuModel.runControl("start")
            }
            Button("Stop Dictation") {
                menuModel.runControl("stop")
            }
            Button("Start Daemon") {
                menuModel.startDaemon()
            }
            Button("Refresh Status") {
                menuModel.refreshStatus()
            }

            Divider()

            Text("Shortcut: \(defaultToggleShortcutLabel)")
                .font(.caption)
                .foregroundStyle(.secondary)

            HStack(spacing: 8) {
                Button("Open Settings") {
                    settingsModel.reload()
                    activateAndFocusSettingsWindow()
                    openWindow(id: "wispr-settings")
                }
                Button("Permissions") {
                    WisprRuntime.shared.openAccessibilitySettings()
                }
            }

            HStack(spacing: 8) {
                Button("View Daemon Log") {
                    let logPath = FileManager.default.homeDirectoryForCurrentUser
                        .appendingPathComponent(".wispr/logs/wisprd.log")
                    NSWorkspace.shared.open(logPath)
                }
                Button("Grant Accessibility") {
                    WisprRuntime.shared.openAccessibilitySettings()
                }
            }

            Divider()

            Button("Quit WisprMac") {
                NSApplication.shared.terminate(nil)
            }
        }
        .padding(12)
        .frame(width: 320)
    }
}

struct SettingsView: View {
    @ObservedObject var model: SettingsModel

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Wispr Settings")
                    .font(.title2)
                    .bold()

                Form {
                    Section("Desktop App") {
                        LabeledContent("Daemon") {
                            Text(model.daemonStatus)
                                .foregroundStyle(.secondary)
                        }

                        LabeledContent("Global Shortcut") {
                            Text(defaultToggleShortcutLabel)
                                .foregroundStyle(.secondary)
                        }

                        HStack(spacing: 10) {
                            Button("Start Daemon") {
                                model.startDaemon()
                            }
                            .disabled(model.busy)

                            Button("Install Launch Agent") {
                                model.installAutostart()
                            }
                            .disabled(model.busy)

                            Button("Remove Launch Agent") {
                                model.removeAutostart()
                            }
                            .disabled(model.busy)
                        }
                    }

                    Section("Permissions") {
                        HStack(spacing: 10) {
                            Button("Request Access") {
                                model.requestMicrophoneAccess()
                            }
                            Button("Accessibility") {
                                model.openAccessibilitySettings()
                            }
                            Button("Microphone") {
                                model.openMicrophoneSettings()
                            }
                            Button("Input Monitoring") {
                                model.openInputMonitoringSettings()
                            }
                        }
                    }

                    Section("Transcription") {
                        Picker("Provider", selection: $model.provider) {
                            Text("Cloud (Deepgram)").tag("deepgram")
                            Text("Local (Whisper)").tag("whisper_local")
                        }

                        TextField("Whisper model", text: $model.whisperModel)
                            .textFieldStyle(.roundedBorder)

                        Button("Save Transcription Settings") {
                            model.saveTranscription()
                        }
                        .disabled(model.busy)
                    }

                    Section("LLM") {
                        TextField("Base URL", text: $model.llmBaseURL)
                            .textFieldStyle(.roundedBorder)
                        TextField("Model", text: $model.llmModel)
                            .textFieldStyle(.roundedBorder)

                        Button("Save LLM Settings") {
                            model.saveLlmSettings()
                        }
                        .disabled(model.busy)
                    }

                    Section("API Keys") {
                        SecureField("Deepgram API key", text: $model.deepgramKey)
                            .textFieldStyle(.roundedBorder)
                        Text(model.deepgramKeyConfigured ? "Deepgram key is configured" : "Deepgram key is not configured")
                            .font(.footnote)
                            .foregroundStyle(.secondary)

                        SecureField("LLM API key", text: $model.llmKey)
                            .textFieldStyle(.roundedBorder)
                        Text(model.llmKeyConfigured ? "LLM key is configured" : "LLM key is not configured")
                            .font(.footnote)
                            .foregroundStyle(.secondary)

                        Button("Save API Keys") {
                            model.saveKeys()
                        }
                        .disabled(model.busy)
                    }

                    Section("Whisper Runtime") {
                        HStack(spacing: 10) {
                            Button("Whisper Status") {
                                model.whisperStatus()
                            }
                            .disabled(model.busy)

                            Button("Install Runtime") {
                                model.installWhisperRuntime()
                            }
                            .disabled(model.busy)
                        }

                        HStack(spacing: 10) {
                            Button("Download Model") {
                                model.downloadWhisperModel()
                            }
                            .disabled(model.busy)

                            Button("Delete Model") {
                                model.deleteWhisperModel()
                            }
                            .disabled(model.busy)

                            Button("Test Model") {
                                model.testWhisperModel()
                            }
                            .disabled(model.busy)
                        }
                    }
                }

                HStack(spacing: 10) {
                    Button("Reload") {
                        model.reload()
                    }
                    .disabled(model.busy)

                    if model.busy {
                        ProgressView()
                            .controlSize(.small)
                    }
                }

                Text(model.statusMessage)
                    .font(.footnote)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .padding(16)
        }
    }
}

final class MenuModel: ObservableObject {
    @Published var lastStatus: String = "Starting Wispr..."
    @Published var lastCommandMessage: String = ""
    @Published var lastCommandWasError: Bool = false

    private let runtime = WisprRuntime.shared
    private var refreshTimer: Timer?
    private var started = false
    private var observer: NSObjectProtocol?

    init() {
        observer = NotificationCenter.default.addObserver(
            forName: .wisprRuntimeDidChange,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.refreshStatus()
        }
    }

    deinit {
        if let observer {
            NotificationCenter.default.removeObserver(observer)
        }
        refreshTimer?.invalidate()
    }

    func start() {
        guard !started else { return }
        started = true
        runtime.ensureDaemonRunningAsync()
        refreshStatus()
        refreshTimer = Timer.scheduledTimer(withTimeInterval: 1.2, repeats: true) { [weak self] _ in
            self?.refreshStatus()
        }
    }

    func startDaemon() {
        DispatchQueue.global(qos: .userInitiated).async {
            let result = self.runtime.ensureDaemonRunning()
            DispatchQueue.main.async {
                self.lastCommandMessage = result.output
                self.refreshStatus()
            }
        }
    }

    func refreshStatus() {
        DispatchQueue.global(qos: .userInitiated).async {
            let result = self.runtime.run(["status"], ensureDaemon: true)
            DispatchQueue.main.async {
                self.lastStatus = self.formatStatusOutput(result)
            }
        }
    }

    func runControl(_ command: String) {
        DispatchQueue.global(qos: .userInitiated).async {
            let result = self.runtime.run([command], ensureDaemon: true)
            DispatchQueue.main.async {
                self.lastCommandMessage = result.output
                self.lastCommandWasError = !result.success
                self.lastStatus = self.formatStatusOutput(result)
            }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
                self.refreshStatus()
            }
        }
    }

    private func formatStatusOutput(_ result: WisprRuntime.CommandResult) -> String {
        guard result.success else {
            return result.output
        }

        guard let data = result.output.data(using: .utf8),
              let status = try? JSONDecoder().decode(DaemonStatusSnapshot.self, from: data)
        else {
            return result.output
        }

        var lines = [
            "State: \(status.state.capitalized)",
            "Provider: \(providerLabel(status.transcriptionProvider))",
        ]

        if let mic = status.currentMic?.displayName, !mic.isEmpty {
            lines.append("Mic: \(mic)")
        }

        if status.state.lowercased() != "listening" {
            lines.append("Dictation is not active")
        } else if let transcriptionState = status.transcriptionState, !transcriptionState.isEmpty {
            lines.append(transcriptionState)
        } else if status.transcriptionReady {
            lines.append("Transcription ready")
        } else {
            lines.append("Transcription not ready")
        }

        if let partial = status.partialTranscript, !partial.isEmpty {
            lines.append("Live: \(partial)")
        }

        let accessOK = status.accessibilityPermission ?? false
        let typingOK = status.typingReady ?? false
        if !accessOK || !typingOK {
            lines.append("!! Accessibility: \(accessOK ? "granted" : "NOT GRANTED")")
            if !accessOK {
                lines.append("   Grant in System Settings > Privacy > Accessibility")
            }
        }

        if let error = status.lastError, !error.isEmpty {
            lines.append("Error: \(error)")
        }
        if let transcriptionError = status.lastTranscriptionError, !transcriptionError.isEmpty {
            lines.append("Transcription: \(transcriptionError)")
        }
        if let llmError = status.lastLlmError, !llmError.isEmpty {
            lines.append("LLM: \(llmError)")
        }

        return lines.joined(separator: "\n")
    }

    private func providerLabel(_ provider: String) -> String {
        switch provider {
        case "deepgram":
            return "Cloud (Deepgram)"
        case "whisper_local":
            return "Local (Whisper)"
        default:
            return provider
        }
    }
}

final class SettingsModel: ObservableObject {
    @Published var provider: String = "deepgram"
    @Published var whisperModel: String = "base.en"
    @Published var llmBaseURL: String = "https://api.openai.com/v1"
    @Published var llmModel: String = "gpt-4o-mini"
    @Published var deepgramKey: String = ""
    @Published var llmKey: String = ""

    @Published var deepgramKeyConfigured: Bool = false
    @Published var llmKeyConfigured: Bool = false
    @Published var daemonStatus: String = "Checking daemon..."
    @Published var statusMessage: String = "Ready"
    @Published var busy: Bool = false

    private let runtime = WisprRuntime.shared

    func reload() {
        runBackground {
            let daemonResult = self.runtime.run(["doctor"], ensureDaemon: false)
            let daemonStatus = daemonResult.output.contains("daemon_status_call=ok")
                ? "Running"
                : "Not running"

            let result = self.runtime.run(["settings-json"], ensureDaemon: false)
            guard result.success else {
                DispatchQueue.main.async {
                    self.daemonStatus = daemonStatus
                }
                return result.output
            }

            guard let data = result.output.data(using: .utf8),
                  let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
            else {
                DispatchQueue.main.async {
                    self.daemonStatus = daemonStatus
                }
                return "Failed to parse settings response."
            }

            DispatchQueue.main.async {
                self.daemonStatus = daemonStatus
                self.provider = (object["provider"] as? String) ?? "deepgram"
                self.whisperModel = (object["whisper_model"] as? String) ?? "base.en"
                self.llmBaseURL = (object["llm_base_url"] as? String) ?? "https://api.openai.com/v1"
                self.llmModel = (object["llm_model"] as? String) ?? "gpt-4o-mini"
                self.deepgramKeyConfigured = (object["deepgram_key_configured"] as? Bool) ?? false
                self.llmKeyConfigured = (object["llm_key_configured"] as? Bool) ?? false
                self.deepgramKey = ""
                self.llmKey = ""
            }

            return "Loaded settings."
        }
    }

    func startDaemon() {
        runBackground {
            let result = self.runtime.ensureDaemonRunning()
            DispatchQueue.main.async {
                self.daemonStatus = result.success ? "Running" : "Not running"
            }
            return result.output
        }
    }

    func installAutostart() {
        runSimple(["install-autostart"])
    }

    func removeAutostart() {
        runSimple(["remove-autostart"])
    }

    func openAccessibilitySettings() {
        runtime.openAccessibilitySettings()
    }

    func requestMicrophoneAccess() {
        runtime.requestMicrophoneAccessIfNeeded()
        statusMessage = "Requested microphone permission."
    }

    func openMicrophoneSettings() {
        runtime.openMicrophoneSettings()
    }

    func openInputMonitoringSettings() {
        runtime.openInputMonitoringSettings()
    }

    func saveTranscription() {
        runBackground {
            let providerResult = self.runtime.run(["set-provider", self.provider], ensureDaemon: false)
            guard providerResult.success else { return providerResult.output }

            let modelResult = self.runtime.run(["set-whisper-model", self.whisperModel], ensureDaemon: false)
            guard modelResult.success else { return modelResult.output }

            DispatchQueue.main.async {
                self.reload()
            }
            return "Saved transcription settings."
        }
    }

    func saveLlmSettings() {
        runBackground {
            let baseResult = self.runtime.run(["set-llm-base-url", self.llmBaseURL], ensureDaemon: false)
            guard baseResult.success else { return baseResult.output }

            let modelResult = self.runtime.run(["set-llm-model", self.llmModel], ensureDaemon: false)
            guard modelResult.success else { return modelResult.output }

            DispatchQueue.main.async {
                self.reload()
            }
            return "Saved LLM settings."
        }
    }

    func saveKeys() {
        runBackground {
            if !self.deepgramKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                let deepgramResult = self.runtime.run(["set-deepgram-key", self.deepgramKey], ensureDaemon: false)
                guard deepgramResult.success else { return deepgramResult.output }
            }

            if !self.llmKey.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                let llmResult = self.runtime.run(["set-llm-key", self.llmKey], ensureDaemon: false)
                guard llmResult.success else { return llmResult.output }
            }

            DispatchQueue.main.async {
                self.reload()
            }
            return "Saved API keys."
        }
    }

    func whisperStatus() {
        runSimple(["whisper-status"])
    }

    func installWhisperRuntime() {
        runSimple(["install-whisper-runtime"])
    }

    func downloadWhisperModel() {
        runSimple(["download-whisper-model", whisperModel])
    }

    func deleteWhisperModel() {
        runSimple(["delete-whisper-model", whisperModel])
    }

    func testWhisperModel() {
        runSimple(["test-whisper-model", whisperModel])
    }

    private func runSimple(_ arguments: [String]) {
        runBackground {
            let result = self.runtime.run(arguments, ensureDaemon: false)
            if result.success {
                DispatchQueue.main.async {
                    self.reload()
                }
            }
            return result.output
        }
    }

    private func runBackground(_ work: @escaping () -> String) {
        DispatchQueue.main.async {
            self.busy = true
        }
        DispatchQueue.global(qos: .userInitiated).async {
            let message = work()
            DispatchQueue.main.async {
                self.statusMessage = message
                self.busy = false
            }
        }
    }
}

final class WisprRuntime {
    static let shared = WisprRuntime()

    struct CommandResult {
        let success: Bool
        let output: String
    }

    private let lock = NSLock()
    private var daemonProcess: Process?

    func ensureDaemonRunningAsync() {
        DispatchQueue.global(qos: .utility).async {
            _ = self.ensureDaemonRunning()
        }
    }

    func toggleFromHotKey() {
        DispatchQueue.global(qos: .userInitiated).async {
            let _ = self.run(["toggle"], ensureDaemon: true)
            NotificationCenter.default.post(name: .wisprRuntimeDidChange, object: nil)
        }
    }

    private func daemonLogURL() -> URL {
        let logDir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".wispr")
            .appendingPathComponent("logs")
        try? FileManager.default.createDirectory(at: logDir, withIntermediateDirectories: true)
        return logDir.appendingPathComponent("wisprd.log")
    }

    func ensureDaemonRunning() -> CommandResult {
        if daemonIsHealthy() {
            return CommandResult(success: true, output: "Wispr daemon is running.")
        }

        guard let wisprdPath = resolveWisprdPath() else {
            return CommandResult(
                success: false,
                output: "wisprd not found. Build with: cargo build --bin wisprd"
            )
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: wisprdPath)
        process.arguments = []
        process.environment = processEnvironment()
        process.currentDirectoryURL = workspaceRootURL()

        let logURL = daemonLogURL()
        FileManager.default.createFile(atPath: logURL.path, contents: nil)
        if let logHandle = try? FileHandle(forWritingTo: logURL) {
            logHandle.seekToEndOfFile()
            process.standardOutput = logHandle
            process.standardError = logHandle
        }

        do {
            try process.run()
            lock.lock()
            daemonProcess = process
            lock.unlock()
        } catch {
            return CommandResult(
                success: false,
                output: "Failed to start wisprd: \(error.localizedDescription)"
            )
        }

        for _ in 0..<20 {
            Thread.sleep(forTimeInterval: 0.15)
            if daemonIsHealthy() {
                NotificationCenter.default.post(name: .wisprRuntimeDidChange, object: nil)
                return CommandResult(success: true, output: "Started wisprd.")
            }
        }

        return CommandResult(success: false, output: "wisprd started but did not become ready.")
    }

    func run(_ arguments: [String], ensureDaemon: Bool) -> CommandResult {
        if ensureDaemon {
            let daemonResult = ensureDaemonRunning()
            if !daemonResult.success {
                return daemonResult
            }
        }

        guard let wisprctlPath = resolveWisprctlPath() else {
            return CommandResult(
                success: false,
                output: "wisprctl not found. Build with: cargo build --bin wisprctl"
            )
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: wisprctlPath)
        process.arguments = arguments
        process.environment = processEnvironment()
        process.currentDirectoryURL = workspaceRootURL()

        let stdout = Pipe()
        let stderr = Pipe()
        process.standardOutput = stdout
        process.standardError = stderr

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return CommandResult(success: false, output: "Failed to run wisprctl: \(error.localizedDescription)")
        }

        let out = String(data: stdout.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let err = String(data: stderr.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let combined = (out + err).trimmingCharacters(in: .whitespacesAndNewlines)
        let result = CommandResult(
            success: process.terminationStatus == 0,
            output: combined.isEmpty ? (process.terminationStatus == 0 ? "OK" : "Command failed.") : combined
        )

        if ["toggle", "start", "stop"].contains(arguments.first ?? "") {
            NotificationCenter.default.post(name: .wisprRuntimeDidChange, object: nil)
        }

        return result
    }

    func openAccessibilitySettings() {
        openSystemSettings("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
    }

    func openMicrophoneSettings() {
        openSystemSettings("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
    }

    func openInputMonitoringSettings() {
        openSystemSettings("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
    }

    func requestMicrophoneAccessIfNeeded() {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            return
        case .notDetermined:
            AVCaptureDevice.requestAccess(for: .audio) { granted in
                DispatchQueue.main.async {
                    NotificationCenter.default.post(name: .wisprRuntimeDidChange, object: nil)
                    if !granted {
                        self.openMicrophoneSettings()
                    }
                }
            }
        case .denied, .restricted:
            DispatchQueue.main.async {
                self.openMicrophoneSettings()
            }
        @unknown default:
            break
        }
    }

    private func openSystemSettings(_ rawURL: String) {
        guard let url = URL(string: rawURL) else { return }
        NSWorkspace.shared.open(url)
    }

    private func daemonIsHealthy() -> Bool {
        let result = runDoctor()
        return result.success && result.output.contains("daemon_status_call=ok")
    }

    private func runDoctor() -> CommandResult {
        guard let wisprctlPath = resolveWisprctlPath() else {
            return CommandResult(success: false, output: "wisprctl not found.")
        }

        let process = Process()
        process.executableURL = URL(fileURLWithPath: wisprctlPath)
        process.arguments = ["doctor"]
        process.environment = processEnvironment()
        process.currentDirectoryURL = workspaceRootURL()

        let stdout = Pipe()
        let stderr = Pipe()
        process.standardOutput = stdout
        process.standardError = stderr

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return CommandResult(success: false, output: error.localizedDescription)
        }

        let out = String(data: stdout.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let err = String(data: stderr.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let combined = (out + err).trimmingCharacters(in: .whitespacesAndNewlines)
        return CommandResult(success: process.terminationStatus == 0, output: combined)
    }

    private func processEnvironment() -> [String: String] {
        var environment = ProcessInfo.processInfo.environment
        let preferredPath = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
        if let currentPath = environment["PATH"], !currentPath.isEmpty {
            if !currentPath.contains("/opt/homebrew/bin") || !currentPath.contains("/usr/local/bin") {
                environment["PATH"] = preferredPath + ":" + currentPath
            }
        } else {
            environment["PATH"] = preferredPath
        }
        if let wisprdPath = resolveWisprdPath() {
            environment["WISPRD_PATH"] = wisprdPath
        }
        return environment
    }

    private func resolveWisprctlPath() -> String? {
        let env = ProcessInfo.processInfo.environment
        if let explicit = env["WISPRCTL_PATH"], isExecutable(explicit) {
            return explicit
        }

        for candidate in binaryCandidates(named: "wisprctl") where isExecutable(candidate) {
            return candidate
        }

        if let whichPath = runCommand("/usr/bin/which", ["wisprctl"]), isExecutable(whichPath) {
            return whichPath
        }

        return nil
    }

    private func resolveWisprdPath() -> String? {
        let env = ProcessInfo.processInfo.environment
        if let explicit = env["WISPRD_PATH"], isExecutable(explicit) {
            return explicit
        }

        for candidate in binaryCandidates(named: "wisprd") where isExecutable(candidate) {
            return candidate
        }

        return nil
    }

    private func binaryCandidates(named name: String) -> [String] {
        var candidates = [String]()
        let fileManager = FileManager.default

        if let executableURL = Bundle.main.executableURL {
            let executableDir = executableURL.deletingLastPathComponent().path
            candidates.append("\(executableDir)/\(name)")
            candidates.append("\(executableDir)/../Resources/bin/\(name)")
        }

        let cwd = fileManager.currentDirectoryPath
        candidates.append(contentsOf: [
            "\(cwd)/target/debug/\(name)",
            "\(cwd)/target/release/\(name)",
            "\(cwd)/../target/debug/\(name)",
            "\(cwd)/../target/release/\(name)",
            "\(cwd)/../../target/debug/\(name)",
            "\(cwd)/../../target/release/\(name)",
        ])

        if let executableURL = Bundle.main.executableURL {
            var searchURL = executableURL.deletingLastPathComponent()
            for _ in 0..<6 {
                candidates.append(searchURL.appendingPathComponent("target/debug/\(name)").path)
                candidates.append(searchURL.appendingPathComponent("target/release/\(name)").path)
                searchURL.deleteLastPathComponent()
            }
        }

        return candidates
    }

    private func workspaceRootURL() -> URL? {
        let fileManager = FileManager.default
        let startingPoints: [URL?] = [
            URL(fileURLWithPath: fileManager.currentDirectoryPath),
            Bundle.main.executableURL?.deletingLastPathComponent(),
        ]

        for startingPoint in startingPoints.compactMap({ $0 }) {
            var current = startingPoint
            for _ in 0..<8 {
                if fileManager.fileExists(atPath: current.appendingPathComponent("Cargo.toml").path) {
                    return current
                }
                current.deleteLastPathComponent()
            }
        }

        return URL(fileURLWithPath: fileManager.currentDirectoryPath)
    }

    private func isExecutable(_ path: String) -> Bool {
        FileManager.default.isExecutableFile(atPath: path)
    }

    private func runCommand(_ executable: String, _ arguments: [String]) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        let stdout = Pipe()
        process.standardOutput = stdout
        process.standardError = Pipe()

        do {
            try process.run()
            process.waitUntilExit()
        } catch {
            return nil
        }

        guard process.terminationStatus == 0 else {
            return nil
        }

        let out = String(data: stdout.fileHandleForReading.readDataToEndOfFile(), encoding: .utf8) ?? ""
        let trimmed = out.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }
}

final class GlobalHotKeyManager {
    static let shared = GlobalHotKeyManager()

    private var hotKeyRef: EventHotKeyRef?
    private var handlerRef: EventHandlerRef?
    private var action: (() -> Void)?

    func registerDefaultToggle(action: @escaping () -> Void) {
        self.action = action
        unregister()

        var eventSpec = EventTypeSpec(
            eventClass: OSType(kEventClassKeyboard),
            eventKind: UInt32(kEventHotKeyPressed)
        )

        InstallEventHandler(
            GetApplicationEventTarget(),
            { _, event, _ in
                guard let event else { return noErr }
                var hotKeyID = EventHotKeyID()
                let status = GetEventParameter(
                    event,
                    EventParamName(kEventParamDirectObject),
                    EventParamType(typeEventHotKeyID),
                    nil,
                    MemoryLayout<EventHotKeyID>.size,
                    nil,
                    &hotKeyID
                )
                if status == noErr, hotKeyID.signature == hotKeySignature {
                    GlobalHotKeyManager.shared.action?()
                }
                return noErr
            },
            1,
            &eventSpec,
            nil,
            &handlerRef
        )

        let hotKeyID = EventHotKeyID(signature: hotKeySignature, id: 1)
        RegisterEventHotKey(
            UInt32(kVK_Space),
            UInt32(controlKey | optionKey),
            hotKeyID,
            GetApplicationEventTarget(),
            0,
            &hotKeyRef
        )
    }

    private func unregister() {
        if let hotKeyRef {
            UnregisterEventHotKey(hotKeyRef)
            self.hotKeyRef = nil
        }

        if let handlerRef {
            RemoveEventHandler(handlerRef)
            self.handlerRef = nil
        }
    }
}

private func activateAndFocusSettingsWindow() {
    NSApplication.shared.activate(ignoringOtherApps: true)
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) {
        focusSettingsWindow()
    }
    DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
        focusSettingsWindow()
    }
}

private func focusSettingsWindow() {
    NSApplication.shared.activate(ignoringOtherApps: true)
    for window in NSApplication.shared.windows where window.title == "Wispr Settings" {
        window.level = .normal
        window.collectionBehavior.remove(.transient)
        window.orderFrontRegardless()
        window.makeKeyAndOrderFront(nil)
        window.makeMain()
    }
}

private struct DaemonStatusSnapshot: Decodable {
    struct CurrentMicSnapshot: Decodable {
        let displayName: String

        enum CodingKeys: String, CodingKey {
            case displayName = "display_name"
        }
    }

    let state: String
    let micReady: Bool?
    let typingReady: Bool?
    let hotkeyReady: Bool?
    let transcriptionProvider: String
    let transcriptionReady: Bool
    let transcriptionState: String?
    let lastTranscriptionError: String?
    let partialTranscript: String?
    let currentMic: CurrentMicSnapshot?
    let lastError: String?
    let lastLlmError: String?
    let accessibilityPermission: Bool?
    let inputMonitoringPermission: Bool?
    let microphonePermission: Bool?

    enum CodingKeys: String, CodingKey {
        case state
        case micReady = "mic_ready"
        case typingReady = "typing_ready"
        case hotkeyReady = "hotkey_ready"
        case transcriptionProvider = "transcription_provider"
        case transcriptionReady = "transcription_ready"
        case transcriptionState = "transcription_state"
        case lastTranscriptionError = "last_transcription_error"
        case partialTranscript = "partial_transcript"
        case currentMic = "current_mic"
        case lastError = "last_error"
        case lastLlmError = "last_llm_error"
        case accessibilityPermission = "accessibility_permission"
        case inputMonitoringPermission = "input_monitoring_permission"
        case microphonePermission = "microphone_permission"
    }
}

private extension Notification.Name {
    static let wisprRuntimeDidChange = Notification.Name("wisprRuntimeDidChange")
}
