// Live lyrics in the Dynamic Island and on the lock screen.
//
// A small widget extension that ships inside the app (added to the Xcode
// project by scripts/add-widget.rb). The app starts the Live Activity and
// sends the current line (plugins/nocturn/ios/Sources/LiveLyrics.swift);
// this file only draws it. The data shape lives in LiveLyricsAttributes.swift,
// which is compiled into both.
import ActivityKit
import SwiftUI
import WidgetKit

@main
struct NocturnLyricsBundle: WidgetBundle {
  var body: some Widget {
    NocturnLyricsLive()
  }
}

private let moon = Color(red: 0.87, green: 0.89, blue: 0.94)

struct NocturnLyricsLive: Widget {
  var body: some WidgetConfiguration {
    ActivityConfiguration(for: NocturnLyricsAttributes.self) { context in
      LockScreenLyrics(state: context.state)
        .activityBackgroundTint(Color.black.opacity(0.6))
        .activitySystemActionForegroundColor(.white)
    } dynamicIsland: { context in
      let s = context.state
      return DynamicIsland {
        DynamicIslandExpandedRegion(.leading) {
          Image(systemName: "moon.stars.fill")
            .font(.title3)
            .foregroundStyle(moon)
            .padding(.leading, 4)
        }
        DynamicIslandExpandedRegion(.trailing) {
          Image(systemName: s.playing ? "waveform" : "pause.fill")
            .font(.title3)
            .foregroundStyle(moon.opacity(0.8))
            .symbolEffect(.variableColor.iterative, isActive: s.playing)
            .padding(.trailing, 4)
        }
        DynamicIslandExpandedRegion(.center) {
          VStack(spacing: 1) {
            Text(s.title).font(.caption.weight(.semibold)).lineLimit(1)
            Text(s.artist).font(.caption2).foregroundStyle(.secondary).lineLimit(1)
          }
        }
        DynamicIslandExpandedRegion(.bottom) {
          VStack(alignment: .leading, spacing: 6) {
            LyricLines(state: s, big: .headline, small: .subheadline)
            SongProgress(state: s)
          }
          .padding(.horizontal, 6)
        }
      } compactLeading: {
        Image(systemName: "moon.stars.fill")
          .foregroundStyle(moon)
      } compactTrailing: {
        Text(compactText(s))
          .font(.caption2.weight(.semibold))
          .foregroundStyle(.white)
          .lineLimit(1)
          .minimumScaleFactor(0.75)
          .frame(maxWidth: 150)
      } minimal: {
        Image(systemName: "music.note")
          .foregroundStyle(moon)
      }
      .keylineTint(moon)
    }
  }
}

private func compactText(_ s: NocturnLyricsAttributes.ContentState) -> String {
  if !s.hasLyrics { return s.title }
  return s.line.isEmpty ? "♪" : s.line
}

struct LyricLines: View {
  let state: NocturnLyricsAttributes.ContentState
  let big: Font
  let small: Font

  var body: some View {
    VStack(alignment: .leading, spacing: 3) {
      if state.hasLyrics {
        Text(state.line.isEmpty ? "♪" : state.line)
          .font(big.weight(.bold))
          .foregroundStyle(.white)
          .lineLimit(2)
          .contentTransition(.opacity)
        if !state.next.isEmpty {
          Text(state.next)
            .font(small.weight(.semibold))
            .foregroundStyle(.white.opacity(0.45))
            .lineLimit(1)
        }
      } else {
        Text(state.title)
          .font(big.weight(.bold))
          .foregroundStyle(.white)
          .lineLimit(1)
        Text(state.artist)
          .font(small)
          .foregroundStyle(.white.opacity(0.55))
          .lineLimit(1)
      }
    }
    .frame(maxWidth: .infinity, alignment: .leading)
  }
}

struct SongProgress: View {
  let state: NocturnLyricsAttributes.ContentState

  var body: some View {
    Group {
      if state.playing, state.end > state.start {
        ProgressView(timerInterval: state.start...state.end, countsDown: false) {
          EmptyView()
        } currentValueLabel: {
          EmptyView()
        }
      } else {
        ProgressView(value: state.progress)
      }
    }
    .progressViewStyle(.linear)
    .tint(moon)
  }
}

struct LockScreenLyrics: View {
  let state: NocturnLyricsAttributes.ContentState

  var body: some View {
    VStack(alignment: .leading, spacing: 8) {
      HStack(spacing: 6) {
        Image(systemName: "moon.stars.fill").foregroundStyle(moon)
        Text(state.hasLyrics ? "\(state.title) · \(state.artist)" : "Nocturn")
          .font(.caption.weight(.semibold))
          .foregroundStyle(.white.opacity(0.7))
          .lineLimit(1)
        Spacer(minLength: 0)
        if !state.playing {
          Image(systemName: "pause.fill").font(.caption).foregroundStyle(.white.opacity(0.6))
        }
      }
      LyricLines(state: state, big: .title3, small: .body)
      SongProgress(state: state)
    }
    .padding(16)
  }
}
