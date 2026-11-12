import AVFoundation
import MediaToolbox
import SwiftUI
import Synchronization

// Beat pulse: the background glows softly with the music.
//
// An audio tap on each song measures how loud the sound is right now (it
// reads the audio on its way to the speaker; nothing is recorded or kept).
// The background compares that with the last few seconds, so it lifts on
// kicks and drops, not just when a song is loud overall.

enum BeatMeter {
  /// Loudness of the latest audio buffer (RMS), as Float bits.
  static let level = Atomic<UInt32>(0)

  static var current: Float { Float(bitPattern: level.load(ordering: .relaxed)) }

  /// Adds the tap to a song once its audio track is known (doesn't delay playback).
  static func attach(to item: AVPlayerItem) {
    Task {
      guard let track = try? await item.asset.loadTracks(withMediaType: .audio).first, let tap = makeTap() else { return }
      let params = AVMutableAudioMixInputParameters(track: track)
      params.audioTapProcessor = tap
      let mix = AVMutableAudioMix()
      mix.inputParameters = [params]
      await MainActor.run { item.audioMix = mix }
    }
  }

  private static func makeTap() -> MTAudioProcessingTap? {
    var callbacks = MTAudioProcessingTapCallbacks(
      version: kMTAudioProcessingTapCallbacksVersion_0,
      clientInfo: nil,
      init: { _, clientInfo, storage in storage.pointee = clientInfo },
      finalize: nil,
      prepare: nil,
      unprepare: nil,
      process: { tap, frames, _, buffers, framesOut, flagsOut in
        guard MTAudioProcessingTapGetSourceAudio(tap, frames, buffers, flagsOut, nil, framesOut) == noErr else { return }
        var sum: Float = 0
        var count = 0
        for buf in UnsafeMutableAudioBufferListPointer(buffers) {
          guard let p = buf.mData?.assumingMemoryBound(to: Float.self) else { continue }
          let n = Int(buf.mDataByteSize) / MemoryLayout<Float>.size
          var i = 0
          while i < n {
            sum += p[i] * p[i]
            i += 4  // every 4th sample is plenty for a level meter
          }
          count += (n + 3) / 4
        }
        if count > 0 { BeatMeter.level.store(sqrt(sum / Float(count)).bitPattern, ordering: .relaxed) }
      })
    var tap: MTAudioProcessingTap?
    guard MTAudioProcessingTapCreate(kCFAllocatorDefault, &callbacks, kMTAudioProcessingTapCreationFlag_PostEffects, &tap) == noErr else { return nil }
    return tap
  }
}

/// Turns the raw level into a 0–1 pulse: quick to rise, slow to fall,
/// relative to the recent average so quiet and loud songs both pulse.
@MainActor
final class BeatEnvelope {
  static let shared = BeatEnvelope()
  private var average: Float = 0.05
  private var value: Float = 0
  private var last = Date()

  func next() -> Double {
    let now = Date()
    let dt = Float(min(0.1, now.timeIntervalSince(last)))
    last = now
    let raw = PlayerEngine.shared.playing ? BeatMeter.current : 0
    average += (raw - average) * min(1, dt * 0.8)
    let lift = max(0, raw / max(0.01, average) - 1.05)
    let target = min(1, lift * 1.6)
    value += (target - value) * min(1, dt * (target > value ? 30 : 5))
    return Double(value)
  }
}

/// A soft glow from the bottom of the screen, in the song's colour, that lifts with the beat.
struct BeatPulseLayer: View {
  let engine: PlayerEngine

  var body: some View {
    let tint = engine.palette.first.map { Color(uiColor: $0) } ?? Color(red: 0.6, green: 0.5, blue: 1)
    TimelineView(.animation(minimumInterval: 1.0 / 60, paused: !engine.playing)) { _ in
      let p = BeatEnvelope.shared.next()
      ZStack {
        RadialGradient(colors: [tint.opacity(0.55 * p), .clear], center: .bottom, startRadius: 0, endRadius: 520)
        RadialGradient(colors: [Color.white.opacity(0.08 * p), .clear], center: .center, startRadius: 0, endRadius: 400)
      }
      .blendMode(.plusLighter)
    }
    .ignoresSafeArea()
    .allowsHitTesting(false)
  }
}
