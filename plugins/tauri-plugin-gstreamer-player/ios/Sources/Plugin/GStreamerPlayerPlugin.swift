import GStreamerBridge
import Tauri
import UIKit
import WebKit

private struct ProcessorRequest: Decodable { var kind = "passthrough" }
private struct OpenRequest: Decodable {
  var uri: String
  var title: String
  var isAudio = false
  var startPositionMs: UInt64?
  var processor = ProcessorRequest()
}
private struct OpenInvokeArgs: Decodable { var request: OpenRequest }
private struct SeekRequest: Decodable { var positionMs: UInt64 }
private struct VolumeRequest: Decodable { var volume: Double }
private struct MutedRequest: Decodable { var muted: Bool }
private struct RateRequest: Decodable { var rate: Double }
private struct LoopingRequest: Decodable { var looping: Bool }
private struct TrackRequest: Decodable { var index: Int32 }
private struct SubtitleUriRequest: Decodable { var uri: String? }

private final class PlayerViewController: UIViewController {
  let mediaView = UIView()
  private let titleLabel = UILabel()
  private let timeline = UISlider()
  private var timer: Timer?
  private var hideWorkItem: DispatchWorkItem?
  private var chromeViews: [UIView] = []
  private var rateIndex = 3
  private let rates = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4]
  var onClose: (() -> Void)?
  var onStatus: ((String) -> Void)?
  var onRate: ((Double) -> Void)?
  var onMuted: ((Bool) -> Void)?
  var onLooping: ((Bool) -> Void)?

  convenience init(title: String) {
    self.init(nibName: nil, bundle: nil)
    titleLabel.text = title
  }

  private func button(_ title: String, _ action: Selector) -> UIButton {
    let result = UIButton(type: .system)
    result.setTitle(title, for: .normal)
    result.setTitleColor(.white, for: .normal)
    result.titleLabel?.font = .systemFont(ofSize: 12, weight: .semibold)
    result.layer.cornerRadius = 12
    result.layer.borderWidth = 0.5
    result.layer.borderColor = UIColor.white.withAlphaComponent(0.24).cgColor
    result.backgroundColor = UIColor.white.withAlphaComponent(0.09)
    result.addTarget(self, action: action, for: .touchUpInside)
    result.addTarget(self, action: #selector(revealChrome), for: .touchUpInside)
    return result
  }

  private func glassView() -> UIVisualEffectView {
    let glass = UIVisualEffectView(effect: UIBlurEffect(style: .systemUltraThinMaterialDark))
    glass.layer.cornerRadius = 22
    glass.layer.cornerCurve = .continuous
    glass.layer.masksToBounds = true
    glass.layer.borderWidth = 0.75
    glass.layer.borderColor = UIColor.white.withAlphaComponent(0.28).cgColor
    return glass
  }

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = .black
    mediaView.backgroundColor = .black
    mediaView.translatesAutoresizingMaskIntoConstraints = false
    view.addSubview(mediaView)

    titleLabel.textColor = .white
    titleLabel.font = .systemFont(ofSize: 13, weight: .semibold)
    titleLabel.lineBreakMode = .byTruncatingTail
    let close = button("×", #selector(closePressed))
    let titleRow = UIStackView(arrangedSubviews: [titleLabel, close])
    titleRow.axis = .horizontal
    titleRow.alignment = .center
    titleRow.spacing = 8
    let titleGlass = glassView()
    titleGlass.translatesAutoresizingMaskIntoConstraints = false
    titleRow.translatesAutoresizingMaskIntoConstraints = false
    titleGlass.contentView.addSubview(titleRow)
    view.addSubview(titleGlass)

    timeline.minimumValue = 0
    timeline.maximumValue = 1
    timeline.addTarget(self, action: #selector(seekReleased), for: [.touchUpInside, .touchUpOutside])
    let controls = UIStackView(arrangedSubviews: [
      button("↶10", #selector(backPressed)),
      button("▶", #selector(playPressed)),
      button("Ⅱ", #selector(pausePressed)),
      button("10↷", #selector(forwardPressed)),
      button("1x", #selector(ratePressed)),
      button("Audio", #selector(audioPressed)),
      button("CC", #selector(subtitlePressed)),
      button("Mute", #selector(mutePressed)),
      button("Repeat", #selector(loopPressed)),
    ])
    controls.axis = .horizontal
    controls.distribution = .fillEqually
    let bottom = UIStackView(arrangedSubviews: [timeline, controls])
    bottom.axis = .vertical
    bottom.spacing = 3
    bottom.translatesAutoresizingMaskIntoConstraints = false
    let bottomGlass = glassView()
    bottomGlass.translatesAutoresizingMaskIntoConstraints = false
    bottomGlass.contentView.addSubview(bottom)
    view.addSubview(bottomGlass)
    chromeViews = [titleGlass, bottomGlass]

    let revealTap = UITapGestureRecognizer(target: self, action: #selector(revealChrome))
    revealTap.cancelsTouchesInView = false
    view.addGestureRecognizer(revealTap)

    NSLayoutConstraint.activate([
      mediaView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
      mediaView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
      mediaView.topAnchor.constraint(equalTo: view.topAnchor),
      mediaView.bottomAnchor.constraint(equalTo: view.bottomAnchor),
      titleGlass.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
      titleGlass.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
      titleGlass.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 10),
      titleGlass.heightAnchor.constraint(equalToConstant: 44),
      titleRow.leadingAnchor.constraint(equalTo: titleGlass.contentView.leadingAnchor, constant: 16),
      titleRow.trailingAnchor.constraint(equalTo: titleGlass.contentView.trailingAnchor, constant: -6),
      titleRow.topAnchor.constraint(equalTo: titleGlass.contentView.topAnchor, constant: 5),
      titleRow.bottomAnchor.constraint(equalTo: titleGlass.contentView.bottomAnchor, constant: -5),
      close.widthAnchor.constraint(equalToConstant: 34),
      bottomGlass.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 16),
      bottomGlass.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -16),
      bottomGlass.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -14),
      bottomGlass.heightAnchor.constraint(equalToConstant: 92),
      bottom.leadingAnchor.constraint(equalTo: bottomGlass.contentView.leadingAnchor, constant: 12),
      bottom.trailingAnchor.constraint(equalTo: bottomGlass.contentView.trailingAnchor, constant: -12),
      bottom.topAnchor.constraint(equalTo: bottomGlass.contentView.topAnchor, constant: 7),
      bottom.bottomAnchor.constraint(equalTo: bottomGlass.contentView.bottomAnchor, constant: -9),
    ])
    timer = Timer.scheduledTimer(withTimeInterval: 0.5, repeats: true) { [weak self] _ in
      guard let self, !self.timeline.isTracking else { return }
      let duration = heri_gstreamer_duration()
      let position = heri_gstreamer_position()
      if duration > 0, position >= 0 { self.timeline.value = Float(position) / Float(duration) }
    }
    revealChrome()
  }

  override func viewDidDisappear(_ animated: Bool) {
    super.viewDidDisappear(animated)
    timer?.invalidate()
    hideWorkItem?.cancel()
  }

  @objc private func revealChrome() {
    hideWorkItem?.cancel()
    UIView.animate(withDuration: 0.16) { self.chromeViews.forEach { $0.alpha = 1 } }
    guard heri_gstreamer_is_playing() else { return }
    let work = DispatchWorkItem { [weak self] in
      guard let self, heri_gstreamer_is_playing() else { return }
      UIView.animate(withDuration: 0.24) { self.chromeViews.forEach { $0.alpha = 0 } }
    }
    hideWorkItem = work
    DispatchQueue.main.asyncAfter(deadline: .now() + 2.8, execute: work)
  }

  func playbackStarted() { revealChrome() }

  @objc private func playPressed() { heri_gstreamer_play(); onStatus?("playing"); revealChrome() }
  @objc private func pausePressed() { heri_gstreamer_pause(); onStatus?("paused"); revealChrome() }
  @objc private func backPressed() {
    _ = heri_gstreamer_seek(UInt64(max(0, heri_gstreamer_position() - 10_000)))
  }
  @objc private func forwardPressed() {
    let duration = heri_gstreamer_duration()
    _ = heri_gstreamer_seek(UInt64(max(0, min(duration, heri_gstreamer_position() + 10_000))))
  }
  @objc private func seekReleased() {
    let duration = heri_gstreamer_duration()
    if duration > 0 { _ = heri_gstreamer_seek(UInt64(Double(duration) * Double(timeline.value))) }
  }
  @objc private func ratePressed(_ sender: UIButton) {
    rateIndex = (rateIndex + 1) % rates.count
    let rate = rates[rateIndex]
    if heri_gstreamer_set_rate(rate) { sender.setTitle("\(rate)x", for: .normal); onRate?(rate) }
  }
  @objc private func audioPressed() {
    let count = heri_gstreamer_audio_track_count()
    if count > 0 { _ = heri_gstreamer_select_audio_track((heri_gstreamer_current_audio_track() + 1) % count) }
  }
  @objc private func subtitlePressed() {
    let count = heri_gstreamer_subtitle_track_count()
    let current = heri_gstreamer_current_subtitle_track()
    _ = heri_gstreamer_select_subtitle_track(current + 1 >= count ? -1 : current + 1)
  }
  @objc private func mutePressed(_ sender: UIButton) {
    sender.isSelected.toggle()
    heri_gstreamer_set_muted(sender.isSelected)
    sender.setTitle(sender.isSelected ? "Unmute" : "Mute", for: .normal)
    onMuted?(sender.isSelected)
  }
  @objc private func loopPressed(_ sender: UIButton) {
    sender.isSelected.toggle()
    heri_gstreamer_set_looping(sender.isSelected)
    sender.setTitle(sender.isSelected ? "Repeating" : "Repeat", for: .normal)
    onLooping?(sender.isSelected)
  }
  @objc private func closePressed() {
    heri_gstreamer_stop()
    dismiss(animated: true)
    onClose?()
  }
}

final class GStreamerPlayerPlugin: Plugin {
  private weak var webView: WKWebView?
  private var playerController: PlayerViewController?
  private var generation: UInt64 = 0
  private var volume = 1.0
  private var muted = false
  private var rate = 1.0
  private var looping = false
  private var status = "stopped"
  private var currentTitle: String?
  private var externalSubtitleUri: String?

  override func load(webview: WKWebView) { self.webView = webview }

  @objc private func open(_ invoke: Invoke) throws {
    let request = try invoke.parseArgs(OpenInvokeArgs.self).request
    guard !request.uri.isEmpty else { invoke.reject("A media URI is required"); return }
    guard request.processor.kind == "passthrough" else { invoke.reject("No ONNX frame processor is installed"); return }
    DispatchQueue.main.async { [weak self] in
      guard let self, let host = self.webView?.window?.rootViewController else {
        invoke.reject("Unable to locate the application view controller"); return
      }
      heri_gstreamer_stop()
      let presentPlayer = { [weak self] in
        guard let self else { invoke.reject("The player was released"); return }
        let controller = PlayerViewController(title: request.title)
        controller.modalPresentationStyle = .fullScreen
        controller.onClose = { [weak self] in self?.clearPlayer() }
        controller.onStatus = { [weak self] in self?.status = $0 }
        controller.onRate = { [weak self] in self?.rate = $0 }
        controller.onMuted = { [weak self] in self?.muted = $0 }
        controller.onLooping = { [weak self] in self?.looping = $0 }
        host.present(controller, animated: true) {
          request.uri.withCString { uriPointer in
            let viewPointer = Unmanaged.passUnretained(controller.mediaView).toOpaque()
            guard heri_gstreamer_open(uriPointer, viewPointer) else {
              controller.dismiss(animated: true); invoke.reject("GStreamer could not open this media source"); return
            }
            self.generation += 1
            self.currentTitle = request.title
            self.playerController = controller
            self.status = "playing"
            self.rate = 1
            self.externalSubtitleUri = nil
            heri_gstreamer_set_volume(self.volume)
            heri_gstreamer_set_muted(self.muted)
            heri_gstreamer_set_looping(self.looping)
            if let start = request.startPositionMs { _ = heri_gstreamer_seek(start) }
            controller.playbackStarted()
            invoke.resolve(["generation": self.generation, "rendererMode": "native-surface"])
          }
        }
      }
      if let existing = self.playerController {
        self.playerController = nil
        existing.dismiss(animated: false, completion: presentPlayer)
      } else {
        presentPlayer()
      }
    }
  }

  private func clearPlayer() {
    playerController = nil; currentTitle = nil; externalSubtitleUri = nil; status = "stopped"; rate = 1
  }
  @objc private func play(_ invoke: Invoke) { heri_gstreamer_play(); status = "playing"; invoke.resolve() }
  @objc private func pause(_ invoke: Invoke) { heri_gstreamer_pause(); status = "paused"; invoke.resolve() }
  @objc private func stop(_ invoke: Invoke) {
    DispatchQueue.main.async { [weak self] in
      heri_gstreamer_stop(); self?.playerController?.dismiss(animated: true); self?.clearPlayer(); invoke.resolve()
    }
  }
  @objc private func seek(_ invoke: Invoke) throws {
    let value = try invoke.parseArgs(SeekRequest.self).positionMs
    heri_gstreamer_seek(value) ? invoke.resolve() : invoke.reject("Seek failed")
  }
  @objc private func setVolume(_ invoke: Invoke) throws {
    let value = try invoke.parseArgs(VolumeRequest.self).volume
    guard (0...1).contains(value) else { invoke.reject("Volume must be between 0 and 1"); return }
    volume = value; heri_gstreamer_set_volume(value); invoke.resolve()
  }
  @objc private func setMuted(_ invoke: Invoke) throws {
    muted = try invoke.parseArgs(MutedRequest.self).muted; heri_gstreamer_set_muted(muted); invoke.resolve()
  }
  @objc private func setRate(_ invoke: Invoke) throws {
    let value = try invoke.parseArgs(RateRequest.self).rate
    guard (0.25...4).contains(value) else { invoke.reject("Playback rate must be between 0.25 and 4"); return }
    if heri_gstreamer_set_rate(value) { rate = value; invoke.resolve() } else { invoke.reject("Rate change failed") }
  }
  @objc private func setLooping(_ invoke: Invoke) throws {
    looping = try invoke.parseArgs(LoopingRequest.self).looping; heri_gstreamer_set_looping(looping); invoke.resolve()
  }
  @objc private func selectAudioTrack(_ invoke: Invoke) throws {
    let index = try invoke.parseArgs(TrackRequest.self).index
    guard index >= 0, index < heri_gstreamer_audio_track_count() else { invoke.reject("Audio track index is out of range"); return }
    heri_gstreamer_select_audio_track(index) ? invoke.resolve() : invoke.reject("Track selection failed")
  }
  @objc private func selectSubtitleTrack(_ invoke: Invoke) throws {
    let index = try invoke.parseArgs(TrackRequest.self).index
    guard index >= -1, index < heri_gstreamer_subtitle_track_count() else { invoke.reject("Subtitle track index is out of range"); return }
    heri_gstreamer_select_subtitle_track(index) ? invoke.resolve() : invoke.reject("Track selection failed")
  }
  @objc private func setSubtitleUri(_ invoke: Invoke) throws {
    let uri = try invoke.parseArgs(SubtitleUriRequest.self).uri
    let success = uri.map { value in value.withCString { heri_gstreamer_set_subtitle_uri($0) } }
      ?? heri_gstreamer_set_subtitle_uri(nil)
    if success { externalSubtitleUri = uri; invoke.resolve() } else { invoke.reject("No media is open") }
  }

  private func tracks(count: Int32, selected: Int32, label: String) -> [[String: Any]] {
    guard count > 0 else { return [] }
    return (0..<count).map { index in [
      "index": index, "label": "\(label) \(index + 1)", "language": NSNull(),
      "codec": NSNull(), "selected": index == selected,
    ] }
  }
  @objc private func getState(_ invoke: Invoke) {
    let position = heri_gstreamer_position(), duration = heri_gstreamer_duration()
    invoke.resolve([
      "generation": generation,
      "status": playerController == nil ? "stopped" : (heri_gstreamer_is_playing() ? "playing" : "paused"),
      "positionMs": position >= 0 ? position : NSNull(), "durationMs": duration >= 0 ? duration : NSNull(),
      "volume": volume, "muted": muted, "rate": rate, "looping": looping,
      "bufferingPercent": NSNull(), "title": currentTitle ?? NSNull(),
      "audioTracks": tracks(count: heri_gstreamer_audio_track_count(), selected: heri_gstreamer_current_audio_track(), label: "Audio"),
      "subtitleTracks": tracks(count: heri_gstreamer_subtitle_track_count(), selected: heri_gstreamer_current_subtitle_track(), label: "Subtitle"),
      "externalSubtitleUri": externalSubtitleUri ?? NSNull(),
    ])
  }
  @objc private func capabilities(_ invoke: Invoke) {
    invoke.resolve([
      "protocolVersion": 2, "frameProcessorApiVersion": 1, "engine": "GStreamer", "nativeVideo": true,
      "playbackRates": [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4],
      "embeddedSubtitles": true, "externalSubtitles": true, "multipleAudioTracks": true,
      "aiProcessors": ["passthrough"],
    ])
  }
}

@_cdecl("init_plugin_gstreamer_player")
func initPlugin() -> Plugin { GStreamerPlayerPlugin() }
