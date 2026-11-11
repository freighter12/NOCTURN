import Foundation

/// A song as the web side sends it (extra fields are ignored).
struct TrackDTO: Codable, Equatable, Sendable {
  let videoId: String
  let title: String
  let artist: String
  let album: String?
  let artwork: String?
  let durationMs: Double?
}

struct ResolvedStream {
  let url: URL
  /// Unix seconds after which the link stops working.
  let expiresAt: TimeInterval
  /// YouTube refuses the link (403) before this moment.
  let availableAt: TimeInterval
  let loudnessDb: Double?
}

enum YouTubeError: LocalizedError {
  case message(String)
  var errorDescription: String? {
    switch self {
    case .message(let m): return m
    }
  }
}

/// Finds playable audio links on the phone itself, so the next song can start
/// while the app is in the background (the web view is asleep then).
///
/// Same method as the Rust side (src-tauri/src/yt.rs): the `VISIONOS` player
/// client returns direct AAC links (itag 140) without JavaScript decoding,
/// provided a visitor ID is sent.
actor YouTube {
  static let shared = YouTube()

  private let safariUA =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15"
  private let visionVersion = "1.02"
  private var visitor: String?
  private var visitorFetched = Date.distantPast
  private var cache: [String: ResolvedStream] = [:]
  private let session: URLSession = {
    let c = URLSessionConfiguration.default
    c.timeoutIntervalForRequest = 20
    c.waitsForConnectivity = false
    return URLSession(configuration: c)
  }()

  func stream(_ videoId: String) async throws -> ResolvedStream {
    if let hit = cache[videoId], hit.expiresAt - Date().timeIntervalSince1970 > 600 {
      return hit
    }
    var visitorId = try await visitorId(force: false)
    var json = try await player(videoId, visitor: visitorId)
    if (json["playabilityStatus"] as? [String: Any])?["status"] as? String == "LOGIN_REQUIRED" {
      visitorId = try await visitorId(force: true)
      json = try await player(videoId, visitor: visitorId)
    }
    let s = try parseStream(json)
    cache[videoId] = s
    if cache.count > 200 { cache.removeAll() }
    return s
  }

  func forget(_ videoId: String) {
    cache.removeValue(forKey: videoId)
  }

  // MARK: visitor ID

  private func visitorId(force: Bool) async throws -> String {
    if !force, let v = visitor, Date().timeIntervalSince(visitorFetched) < 6 * 3600 {
      return v
    }
    var req = URLRequest(url: URL(string: "https://www.youtube.com/sw.js_data")!)
    req.setValue(safariUA, forHTTPHeaderField: "User-Agent")
    let (data, _) = try await session.data(for: req)
    guard var text = String(data: data, encoding: .utf8) else { throw YouTubeError.message("YouTube sent no visitor ID") }
    if text.hasPrefix(")]}'") { text.removeFirst(4) }
    let root = try JSONSerialization.jsonObject(with: Data(text.utf8))
    guard let a0 = root as? [Any], let a1 = a0.first as? [Any], a1.count > 2,
      let a2 = a1[2] as? [Any], let a3 = a2.first as? [Any], let a4 = a3.first as? [Any],
      a4.count > 13, let id = a4[13] as? String, !id.isEmpty
    else { throw YouTubeError.message("YouTube sent no visitor ID") }
    visitor = id
    visitorFetched = Date()
    return id
  }

  // MARK: player

  private func player(_ videoId: String, visitor: String) async throws -> [String: Any] {
    var req = URLRequest(url: URL(string: "https://www.youtube.com/youtubei/v1/player?prettyPrint=false")!)
    req.httpMethod = "POST"
    req.setValue("application/json", forHTTPHeaderField: "Content-Type")
    req.setValue(safariUA, forHTTPHeaderField: "User-Agent")
    req.setValue("101", forHTTPHeaderField: "X-YouTube-Client-Name")
    req.setValue(visionVersion, forHTTPHeaderField: "X-YouTube-Client-Version")
    req.setValue(visitor, forHTTPHeaderField: "X-Goog-Visitor-Id")
    req.setValue("https://www.youtube.com", forHTTPHeaderField: "Origin")
    let body: [String: Any] = [
      "context": [
        "client": [
          "clientName": "VISIONOS", "clientVersion": visionVersion,
          "deviceMake": "Apple", "deviceModel": "RealityDevice17,1",
          "userAgent": safariUA, "osName": "visionOS", "osVersion": "26.5.23O471",
          "hl": "en", "gl": "GB", "visitorData": visitor,
        ]
      ],
      "videoId": videoId,
      "contentCheckOk": true,
      "racyCheckOk": true,
    ]
    req.httpBody = try JSONSerialization.data(withJSONObject: body)
    let (data, resp) = try await session.data(for: req)
    if let http = resp as? HTTPURLResponse, http.statusCode == 429 {
      throw YouTubeError.message("YouTube is limiting requests right now")
    }
    guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
      throw YouTubeError.message("YouTube's answer couldn't be read")
    }
    return json
  }

  private func parseStream(_ json: [String: Any]) throws -> ResolvedStream {
    let ps = json["playabilityStatus"] as? [String: Any]
    let status = ps?["status"] as? String ?? ""
    guard status == "OK" else {
      let reason = (ps?["reason"] as? String) ?? ""
      throw YouTubeError.message(status == "LOGIN_REQUIRED" ? "YouTube wants a sign-in for this song" : "This song can't be played \(reason)")
    }
    let formats = ((json["streamingData"] as? [String: Any])?["adaptiveFormats"] as? [[String: Any]]) ?? []
    let aac = formats.filter { ($0["mimeType"] as? String)?.hasPrefix("audio/mp4") == true && $0["url"] is String }
    guard let pick = aac.first(where: { ($0["itag"] as? Int) == 140 }) ?? aac.max(by: { ($0["bitrate"] as? Int ?? 0) < ($1["bitrate"] as? Int ?? 0) }),
      let urlString = pick["url"] as? String, let url = URL(string: urlString)
    else { throw YouTubeError.message("YouTube sent no playable audio for this song") }
    let items = URLComponents(url: url, resolvingAgainstBaseURL: false)?.queryItems ?? []
    let num = { (k: String) in items.first(where: { $0.name == k })?.value.flatMap(TimeInterval.init) }
    let loudness = ((json["playerConfig"] as? [String: Any])?["audioConfig"] as? [String: Any])?["loudnessDb"] as? Double
    return ResolvedStream(
      url: url,
      expiresAt: num("expire") ?? Date().timeIntervalSince1970 + 3600,
      availableAt: num("available_at") ?? 0,
      loudnessDb: loudness)
  }

  // MARK: radio (keeps the music going when the queue runs out in the background)

  func radio(after videoId: String) async throws -> [TrackDTO] {
    var req = URLRequest(url: URL(string: "https://music.youtube.com/youtubei/v1/next?prettyPrint=false")!)
    req.httpMethod = "POST"
    req.setValue("application/json", forHTTPHeaderField: "Content-Type")
    req.setValue(safariUA, forHTTPHeaderField: "User-Agent")
    req.setValue("https://music.youtube.com", forHTTPHeaderField: "Origin")
    req.setValue("https://music.youtube.com/", forHTTPHeaderField: "Referer")
    req.setValue("CONSENT=YES+1; SOCS=CAI", forHTTPHeaderField: "Cookie")
    let body: [String: Any] = [
      "context": ["client": ["clientName": "WEB_REMIX", "clientVersion": "1.20251015.03.00", "hl": "en", "gl": "GB"]],
      "videoId": videoId,
      "playlistId": "RDAMVM\(videoId)",
      "isAudioOnly": true,
    ]
    req.httpBody = try JSONSerialization.data(withJSONObject: body)
    let (data, _) = try await session.data(for: req)
    let root = try JSONSerialization.jsonObject(with: data)
    var out: [TrackDTO] = []
    YouTube.walk(root) { key, value in
      guard key == "playlistPanelVideoRenderer", let r = value as? [String: Any] else { return false }
      guard let vid = r["videoId"] as? String, vid.count == 11, !out.contains(where: { $0.videoId == vid }) else { return true }
      let title = YouTube.text(r["title"])
      guard !title.isEmpty else { return true }
      let byline = YouTube.text(r["longBylineText"])
      let artist = byline.components(separatedBy: " • ").first ?? ""
      let thumbs = ((r["thumbnail"] as? [String: Any])?["thumbnails"] as? [[String: Any]]) ?? []
      let art = (thumbs.last?["url"] as? String).map(YouTube.upsize)
      out.append(TrackDTO(videoId: vid, title: title, artist: artist, album: nil, artwork: art, durationMs: YouTube.parseLen(YouTube.text(r["lengthText"]))))
      return true
    }
    return out
  }

  // MARK: helpers

  static func walk(_ v: Any, _ f: (String, Any) -> Bool) {
    if let d = v as? [String: Any] {
      for (k, x) in d where !f(k, x) { walk(x, f) }
    } else if let a = v as? [Any] {
      for x in a { walk(x, f) }
    }
  }

  static func text(_ v: Any?) -> String {
    guard let d = v as? [String: Any] else { return "" }
    if let runs = d["runs"] as? [[String: Any]] { return runs.compactMap { $0["text"] as? String }.joined() }
    return d["simpleText"] as? String ?? ""
  }

  static func parseLen(_ s: String) -> Double? {
    let parts = s.split(separator: ":").map { Double($0.trimmingCharacters(in: .whitespaces)) }
    guard parts.count >= 2, !parts.contains(where: { $0 == nil }) else { return nil }
    return parts.reduce(0.0) { $0 * 60 + ($1 ?? 0) } * 1000
  }

  static func upsize(_ url: String) -> String {
    var u = url.hasPrefix("//") ? "https:" + url : url
    if u.contains("googleusercontent.com"), let i = u.lastIndex(of: "=") {
      u = String(u[..<i]) + "=w544-h544-l90-rj"
    }
    return u
  }
}
