# Adds the live-lyrics widget extension (mobile/widget) to the Xcode project
# that `tauri ios init` generates, then regenerates the project.
#
#   ruby scripts/add-widget.rb            (run from mobile/, after `tauri ios init`)
#
# Tauri only generates project.yml during init (never during build), so the
# extra target stays in place for `tauri ios build`.
require "yaml"

dir = ARGV[0] || "src-tauri/gen/apple"
spec = File.join(dir, "project.yml")
y = YAML.load_file(spec)

targets = y.fetch("targets")
app_name, app = targets.find { |name, _| name.end_with?("_iOS") }
abort "no _iOS app target in #{spec}" unless app

bundle = y.dig("settingGroups", "app", "base", "PRODUCT_BUNDLE_IDENTIFIER") || "app.nocturn.music"
props = app.dig("info", "properties") || {}
ios = y.dig("options", "deploymentTarget", "iOS").to_s

# paths are relative to gen/apple
targets["NocturnLyrics"] = {
  "type" => "app-extension",
  "platform" => "iOS",
  "deploymentTarget" => ios,
  "sources" => [
    { "path" => "../../../widget" },
    { "path" => "../../../plugins/nocturn/ios/Sources/LiveLyricsAttributes.swift" },
  ],
  "info" => {
    "path" => "NocturnLyrics/Info.plist",
    "properties" => {
      "CFBundleDisplayName" => "Nocturn Lyrics",
      "CFBundleShortVersionString" => props["CFBundleShortVersionString"].to_s,
      "CFBundleVersion" => props["CFBundleVersion"].to_s,
      "NSExtension" => { "NSExtensionPointIdentifier" => "com.apple.widgetkit-extension" },
    },
  },
  "settings" => {
    "base" => {
      "PRODUCT_BUNDLE_IDENTIFIER" => "#{bundle}.lyrics",
      "PRODUCT_NAME" => "NocturnLyrics",
      "SWIFT_VERSION" => "5.0",
      "TARGETED_DEVICE_FAMILY" => "1,2",
      "SKIP_INSTALL" => "YES",
      "ENABLE_BITCODE" => "NO",
      "EXCLUDED_ARCHS[sdk=iphoneos*]" => "x86_64",
      "LD_RUNPATH_SEARCH_PATHS" => "$(inherited) @executable_path/Frameworks @executable_path/../../Frameworks",
    },
  },
  "dependencies" => [
    { "sdk" => "WidgetKit.framework" },
    { "sdk" => "SwiftUI.framework" },
    { "sdk" => "ActivityKit.framework" },
  ],
}

deps = (app["dependencies"] ||= [])
deps << { "target" => "NocturnLyrics", "embed" => true } unless deps.any? { |d| d["target"] == "NocturnLyrics" }

File.write(spec, y.to_yaml)
puts "Added NocturnLyrics extension to #{app_name} (#{bundle}.lyrics)"
system("xcodegen", "generate", "--spec", spec) or abort "xcodegen failed"
