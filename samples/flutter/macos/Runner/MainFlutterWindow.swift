import Cocoa
import FlutterMacOS

class MainFlutterWindow: NSWindow {
  override func awakeFromNib() {
    let flutterViewController = FlutterViewController()
    self.contentViewController = flutterViewController
    // The sidebar and a table of objects side by side need more room than
    // the template's window.
    self.setContentSize(NSSize(width: 1360, height: 860))
    self.contentMinSize = NSSize(width: 960, height: 600)
    self.center()

    RegisterGeneratedPlugins(registry: flutterViewController)

    super.awakeFromNib()
  }
}
