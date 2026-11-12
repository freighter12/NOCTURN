import AVFoundation
import UIKit

// Scans the QR code NOCTURN on the PC shows (Music › Sources › Sync with
// iPhone). A full-screen camera with a frame to aim at; the first QR code
// seen is returned. Nothing is recorded or saved.

@MainActor
final class QRScannerController: UIViewController, @preconcurrency AVCaptureMetadataOutputObjectsDelegate {
  var onResult: ((String?) -> Void)?
  private let session = AVCaptureSession()
  private var finished = false

  override func viewDidLoad() {
    super.viewDidLoad()
    view.backgroundColor = .black

    guard let device = AVCaptureDevice.default(for: .video), let input = try? AVCaptureDeviceInput(device: device), session.canAddInput(input) else {
      showMessage("The camera isn't available.")
      return
    }
    session.addInput(input)
    let output = AVCaptureMetadataOutput()
    guard session.canAddOutput(output) else {
      showMessage("The camera isn't available.")
      return
    }
    session.addOutput(output)
    output.setMetadataObjectsDelegate(self, queue: .main)
    output.metadataObjectTypes = [.qr]

    let preview = AVCaptureVideoPreviewLayer(session: session)
    preview.videoGravity = .resizeAspectFill
    preview.frame = view.bounds
    view.layer.addSublayer(preview)

    // the frame to aim at
    let side = min(view.bounds.width, view.bounds.height) * 0.66
    let frame = UIView(frame: CGRect(x: (view.bounds.width - side) / 2, y: (view.bounds.height - side) / 2 - 30, width: side, height: side))
    frame.layer.borderColor = UIColor.white.withAlphaComponent(0.9).cgColor
    frame.layer.borderWidth = 3
    frame.layer.cornerRadius = 24
    view.addSubview(frame)

    let label = UILabel()
    label.text = "Point at the code on your PC\n(NOCTURN › Music › Sources › Sync with iPhone)"
    label.numberOfLines = 0
    label.textAlignment = .center
    label.textColor = .white
    label.font = .systemFont(ofSize: 16, weight: .semibold)
    label.frame = CGRect(x: 24, y: frame.frame.maxY + 24, width: view.bounds.width - 48, height: 60)
    view.addSubview(label)

    var config = UIButton.Configuration.glass()
    config.title = "Cancel"
    config.cornerStyle = .capsule
    let cancel = UIButton(configuration: config, primaryAction: UIAction { [weak self] _ in self?.finish(nil) })
    cancel.frame = CGRect(x: (view.bounds.width - 140) / 2, y: view.bounds.height - view.safeAreaInsets.bottom - 110, width: 140, height: 50)
    view.addSubview(cancel)

    let s = session
    DispatchQueue.global(qos: .userInitiated).async { s.startRunning() }
  }

  private func showMessage(_ text: String) {
    let label = UILabel(frame: view.bounds.insetBy(dx: 30, dy: 0))
    label.text = text
    label.textColor = .white
    label.textAlignment = .center
    label.numberOfLines = 0
    view.addSubview(label)
    DispatchQueue.main.asyncAfter(deadline: .now() + 1.6) { [weak self] in self?.finish(nil) }
  }

  func metadataOutput(_ output: AVCaptureMetadataOutput, didOutput objects: [AVMetadataObject], from connection: AVCaptureConnection) {
    guard let code = objects.compactMap({ ($0 as? AVMetadataMachineReadableCodeObject)?.stringValue }).first else { return }
    UINotificationFeedbackGenerator().notificationOccurred(.success)
    finish(code)
  }

  private func finish(_ code: String?) {
    guard !finished else { return }
    finished = true
    let s = session
    DispatchQueue.global(qos: .userInitiated).async { s.stopRunning() }
    dismiss(animated: true) { [onResult] in onResult?(code) }
  }
}

enum QRScanner {
  /// Asks for the camera if needed, then shows the scanner. nil = cancelled or no camera.
  @MainActor
  static func scan(from presenter: UIViewController, _ done: @escaping (String?, String?) -> Void) {
    switch AVCaptureDevice.authorizationStatus(for: .video) {
    case .authorized:
      present(from: presenter, done)
    case .notDetermined:
      AVCaptureDevice.requestAccess(for: .video) { ok in
        DispatchQueue.main.async {
          MainActor.assumeIsolated {
            if ok { present(from: presenter, done) } else { done(nil, "Nocturn needs the camera to scan the code. Turn it on in Settings › Nocturn.") }
          }
        }
      }
    default:
      done(nil, "Nocturn needs the camera to scan the code. Turn it on in Settings › Nocturn › Camera.")
    }
  }

  @MainActor
  private static func present(from presenter: UIViewController, _ done: @escaping (String?, String?) -> Void) {
    let vc = QRScannerController()
    vc.modalPresentationStyle = .fullScreen
    vc.onResult = { code in done(code, nil) }
    presenter.present(vc, animated: true)
  }
}
