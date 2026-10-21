#!/usr/bin/env swift
import Foundation
import Vision
import CoreGraphics
import ImageIO

guard CommandLine.arguments.count >= 2 else {
  fputs("usage: wechat_ocr <png>\n", stderr)
  exit(2)
}
let path = CommandLine.arguments[1]
let url = URL(fileURLWithPath: path)
guard let src = CGImageSourceCreateWithURL(url as CFURL, nil),
      let cg = CGImageSourceCreateImageAtIndex(src, 0, nil) else {
  fputs("bad image\n", stderr)
  exit(3)
}

let request = VNRecognizeTextRequest()
request.recognitionLevel = .fast
request.usesLanguageCorrection = true
if #available(macOS 13.0, *) {
  request.recognitionLanguages = ["zh-Hans", "zh-Hant", "en-US"]
  request.automaticallyDetectsLanguage = true
}

let handler = VNImageRequestHandler(cgImage: cg, options: [:])
do {
  try handler.perform([request])
} catch {
  fputs("vision failed: \(error)\n", stderr)
  exit(4)
}

var lines: [(CGFloat, String)] = []
for obs in request.results ?? [] {
  guard let top = try? obs.topCandidates(1).first else { continue }
  let s = top.string.trimmingCharacters(in: .whitespacesAndNewlines)
  guard !s.isEmpty else { continue }
  let hasCJK = s.unicodeScalars.contains { $0.value >= 0x4E00 && $0.value <= 0x9FFF }
  guard hasCJK else { continue }
  lines.append((obs.boundingBox.origin.y, s))
}
lines.sort { $0.0 < $1.0 }
// Prefer bottom-of-crop lines (composer).
let text = lines.suffix(3).map(\.1).joined()
print(text)
