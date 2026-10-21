#!/usr/bin/env swift
import Carbon
import Foundation

let pathCandidates = [
  "/Library/Input Methods/DeskCapIME.app",
  NSHomeDirectory() + "/Library/Input Methods/DeskCapIME.app",
]
guard let path = pathCandidates.first(where: { FileManager.default.fileExists(atPath: $0) }) else {
  print("ERROR: DeskCapIME.app not found in Input Methods")
  exit(1)
}
let status = TISRegisterInputSource(URL(fileURLWithPath: path) as CFURL)
print("TISRegisterInputSource status=\(status) path=\(path)")

let list = TISCreateInputSourceList(nil, true).takeRetainedValue() as! [TISInputSource]
var found = 0
for src in list {
  guard let cfid = TISGetInputSourceProperty(src, kTISPropertyInputSourceID) else { continue }
  let id = Unmanaged<CFString>.fromOpaque(cfid).takeUnretainedValue() as String
  guard id.contains("deskcap.inputmethod") || id.contains("inputmethod.symi") else { continue }
  found += 1
  let namePtr = TISGetInputSourceProperty(src, kTISPropertyLocalizedName)
  let name = namePtr.map { Unmanaged<CFString>.fromOpaque($0).takeUnretainedValue() as String } ?? "?"
  let enPtr = TISGetInputSourceProperty(src, kTISPropertyInputSourceIsEnabled)
  let enabled = enPtr.map { CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque($0).takeUnretainedValue()) } ?? false
  let selPtr = TISGetInputSourceProperty(src, kTISPropertyInputSourceIsSelectCapable)
  let selectable = selPtr.map { CFBooleanGetValue(Unmanaged<CFBoolean>.fromOpaque($0).takeUnretainedValue()) } ?? false
  print("FOUND id=\(id) name=\(name) enabled=\(enabled) selectable=\(selectable)")
  // Only enable the real pinyin mode (not parent, not ghost *.deskcap.pinyin)
  if id == "com.office.deskcap.inputmethod.symi.pinyin" {
    print("  TISEnableInputSource -> \(TISEnableInputSource(src))")
  }
}
print("deskcap sources found=\(found)")
