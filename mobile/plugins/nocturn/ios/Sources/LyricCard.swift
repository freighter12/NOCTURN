import SwiftUI
import UIKit

// Lyric cards: press and hold a line in Now Playing › Share as Card. A
// story-sized picture (9:16) of the line with the song's artwork, to save
// or post. Made on the phone; nothing is uploaded.

struct LyricCardRequest: Identifiable {
  let id = UUID()
  let line: String
  let translation: String?
  let title: String
  let artist: String
  let artwork: UIImage?
  let palette: [UIColor]
}

/// The card itself, drawn at 360 × 640 points (rendered at 3× = 1080 × 1920).
struct LyricCardView: View {
  let request: LyricCardRequest
  /// "artwork" (blurred cover) | "colour" (the cover's colours) | "night" (dark)
  let style: String

  private var colors: [Color] {
    let p = request.palette.map { Color(uiColor: $0) }
    return p.isEmpty ? [Color(red: 0.25, green: 0.2, blue: 0.5), Color(red: 0.1, green: 0.15, blue: 0.35)] : p
  }

  var body: some View {
    ZStack {
      background
      VStack(alignment: .leading, spacing: 0) {
        Spacer(minLength: 0)
        if let art = request.artwork {
          Image(uiImage: art)
            .resizable()
            .scaledToFill()
            .frame(width: 92, height: 92)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
            .shadow(color: .black.opacity(0.45), radius: 18, y: 10)
            .padding(.bottom, 26)
        }
        Text("\u{201C}\(request.line)\u{201D}")
          .font(.system(size: request.line.count > 70 ? 26 : 32, weight: .heavy))
          .foregroundStyle(.white)
          .fixedSize(horizontal: false, vertical: true)
          .shadow(color: .black.opacity(0.25), radius: 10)
        if let tr = request.translation {
          Text(tr)
            .font(.system(size: 17, weight: .semibold))
            .foregroundStyle(.white.opacity(0.72))
            .padding(.top, 10)
            .fixedSize(horizontal: false, vertical: true)
        }
        VStack(alignment: .leading, spacing: 3) {
          Text(request.title).font(.system(size: 17, weight: .bold))
          Text(request.artist).font(.system(size: 15, weight: .medium)).opacity(0.75)
        }
        .foregroundStyle(.white)
        .lineLimit(1)
        .padding(.top, 26)
        Spacer(minLength: 0)
        HStack(spacing: 7) {
          Moon().frame(width: 15, height: 15)
          Text("NOCTURN").font(.system(size: 12, weight: .bold)).tracking(3)
        }
        .foregroundStyle(.white.opacity(0.8))
      }
      .padding(.horizontal, 34)
      .padding(.vertical, 44)
      .frame(maxWidth: .infinity, alignment: .leading)
    }
    .frame(width: 360, height: 640)
    .clipped()
  }

  @ViewBuilder private var background: some View {
    switch style {
    case "colour":
      LinearGradient(colors: colors + [colors[0].opacity(0.6)], startPoint: .topLeading, endPoint: .bottomTrailing)
        .overlay(Color.black.opacity(0.28))
    case "night":
      LinearGradient(colors: [Color(red: 0.05, green: 0.06, blue: 0.12), Color(red: 0.01, green: 0.01, blue: 0.03)], startPoint: .top, endPoint: .bottom)
        .overlay(alignment: .topTrailing) {
          Circle().fill(colors[0].opacity(0.35)).frame(width: 320).blur(radius: 90).offset(x: 120, y: -100)
        }
    default:
      ZStack {
        Color.black
        if let art = request.artwork {
          Image(uiImage: art).resizable().scaledToFill().frame(width: 360, height: 640).blur(radius: 40).opacity(0.9)
        }
        LinearGradient(colors: [.black.opacity(0.15), .black.opacity(0.55)], startPoint: .top, endPoint: .bottom)
      }
    }
  }
}

/// The crescent from the Nocturn logo.
private struct Moon: View {
  var body: some View {
    Canvas { c, size in
      let r = min(size.width, size.height) / 2
      let full = Path(ellipseIn: CGRect(x: 0, y: 0, width: r * 2, height: r * 2))
      var layer = c
      layer.clip(to: full)
      layer.fill(full, with: .color(.white))
      layer.blendMode = .destinationOut
      layer.fill(Path(ellipseIn: CGRect(x: r * 0.55, y: -r * 0.35, width: r * 1.8, height: r * 1.8)), with: .color(.black))
    }
  }
}

struct LyricCardSheet: View {
  let request: LyricCardRequest
  @State private var style = "artwork"
  @Environment(\.dismiss) private var dismiss

  private func render() -> UIImage? {
    let r = ImageRenderer(content: LyricCardView(request: request, style: style))
    r.scale = 3
    return r.uiImage
  }

  var body: some View {
    NavigationStack {
      VStack(spacing: 18) {
        LyricCardView(request: request, style: style)
          .scaleEffect(0.78)
          .frame(width: 360 * 0.78, height: 640 * 0.78)
          .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
          .shadow(color: .black.opacity(0.5), radius: 24, y: 12)
        Picker("Background", selection: $style) {
          Text("Artwork").tag("artwork")
          Text("Colour").tag("colour")
          Text("Night").tag("night")
        }
        .pickerStyle(.segmented)
        .padding(.horizontal, 30)
        if let img = render() {
          let picture = Image(uiImage: img)
          ShareLink(item: picture, preview: SharePreview("\(request.title) lyrics", image: picture)) {
            Label("Share", systemImage: "square.and.arrow.up")
              .font(.headline)
              .frame(maxWidth: .infinity)
              .padding(.vertical, 6)
          }
          .buttonStyle(.glassProminent)
          .padding(.horizontal, 30)
        }
      }
      .padding(.top, 8)
      .frame(maxWidth: .infinity, maxHeight: .infinity)
      .background(Color(red: 0.04, green: 0.04, blue: 0.06))
      .toolbar {
        ToolbarItem(placement: .cancellationAction) {
          Button("Close", systemImage: "xmark") { dismiss() }
        }
      }
      .navigationTitle("Lyric Card")
      .navigationBarTitleDisplayMode(.inline)
    }
    .preferredColorScheme(.dark)
  }
}
