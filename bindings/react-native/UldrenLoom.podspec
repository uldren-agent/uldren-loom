require "json"

package = JSON.parse(File.read(File.join(__dir__, "package.json")))

Pod::Spec.new do |s|
  s.name         = "UldrenLoom"
  s.version      = package["version"]
  s.summary      = package["description"]
  s.license      = { :type => "BUSL-1.1" }
  s.authors      = package["author"]
  s.homepage     = "https://github.com/uldrenai/uldren-loom"
  s.platforms    = { :ios => "13.4" }
  s.source       = { :git => "https://github.com/uldrenai/uldren-loom.git" }
  s.source_files = "ios/**/*.{h,mm}"

  # The Uldren Loom C ABI: header (repo include/) + the prebuilt static lib for the iOS targets
  # (build with cargo for aarch64-apple-ios and the simulator targets, lipo/xcframework as needed).
  s.vendored_libraries    = "ios/libuldren_loom.a"
  s.pod_target_xcconfig   = { "HEADER_SEARCH_PATHS" => "\"$(PODS_TARGET_SRCROOT)/../../include\"" }

  # Wires the TurboModule (new architecture) deps.
  install_modules_dependencies(s)
end
