#
# To learn more about a Podspec see http://guides.cocoapods.org/syntax/podspec.html.
#
Pod::Spec.new do |s|
  s.name             = 'entidb_flutter'
  s.version          = '2.0.0-alpha.2'
  s.summary          = 'EntiDB embedded database for Flutter'
  s.description      = <<-DESC
Flutter plugin for EntiDB - an embedded entity database with ACID transactions and CBOR storage.
                       DESC
  s.homepage         = 'https://github.com/Tembocs/entidb'
  s.license          = { :type => 'MIT', :file => '../LICENSE' }
  s.author           = { 'Tembocs' => 'tembocs@example.com' }
  s.source           = { :path => '.' }
  s.source_files     = 'Classes/**/*'
  
  s.ios.deployment_target = '12.0'
  s.ios.vendored_frameworks = 'Frameworks/entidb_ffi.xcframework'
  
  # Flutter.framework does not contain an i386 slice
  s.pod_target_xcconfig = { 
    'DEFINES_MODULE' => 'YES',
    'EXCLUDED_ARCHS[sdk=iphonesimulator*]' => 'i386'
  }
  s.swift_version = '5.0'
end
