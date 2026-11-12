import Foundation

/// The Russian keyboard's own word list: completions ranked by how common a
/// word is, and the spellings with «ё». `UITextChecker` has neither for Russian.
/// Built from the Android word list by `Scripts/build_keyboard_words.py`.
final class WordFrequencyList {
  /// Lowercased and sorted, for prefix search; `words` and `frequencies` follow the same order.
  private let keys: [String]
  /// The spelling with capitals (Москва), nil for the many words that are their key.
  private let words: [String?]
  private let frequencies: [UInt8]
  /// е-spelling -> ё-spelling where the е-spelling is not a word itself (еще -> ещё).
  private let yoSpellings: [String: String]
  /// е-spelling -> ё-words of the list, ambiguous ones included (все -> всё).
  private let yoWords: [String: [String]]

  init(wordsTSV: String, yoTSV: String) {
    var entries: [(key: String, word: String, frequency: UInt8)] = []
    var yoWords: [String: [String]] = [:]
    for line in wordsTSV.split(separator: "\n") {
      let parts = line.split(separator: "\t")
      guard parts.count == 2, let frequency = UInt8(parts[1]) else { continue }
      let word = String(parts[0])
      let key = word.lowercased()
      entries.append((key, word, frequency))
      if key.contains("ё") {
        yoWords[key.replacingOccurrences(of: "ё", with: "е"), default: []].append(key)
      }
    }
    // The build script writes the list sorted; sort only if it was edited by hand.
    if zip(entries, entries.dropFirst()).contains(where: { $0.key > $1.key }) {
      entries.sort { $0.key < $1.key }
    }
    keys = entries.map(\.key)
    words = entries.map { $0.word == $0.key ? nil : $0.word }
    frequencies = entries.map(\.frequency)
    self.yoWords = yoWords

    var yoSpellings: [String: String] = [:]
    for line in yoTSV.split(separator: "\n") {
      let parts = line.split(separator: "\t")
      guard parts.count == 2 else { continue }
      yoSpellings[String(parts[0])] = String(parts[1])
    }
    self.yoSpellings = yoSpellings
  }

  /// The bundled Russian list; nil in a keyboard that does not ship it.
  static func russian(in bundle: Bundle = .main) -> WordFrequencyList? {
    guard let wordsURL = bundle.url(forResource: "ru-words", withExtension: "tsv"),
      let words = try? String(contentsOf: wordsURL, encoding: .utf8)
    else { return nil }
    let yo = bundle.url(forResource: "ru-yo", withExtension: "tsv")
      .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
    return WordFrequencyList(wordsTSV: words, yoTSV: yo)
  }

  var count: Int { keys.count }

  func contains(_ word: String) -> Bool {
    let key = word.lowercased()
    let index = lowerBound(of: key)
    return index < keys.count && keys[index] == key
  }

  /// The most common words that start with `prefix` and are longer than it.
  func completions(for prefix: String, limit: Int) -> [String] {
    let key = prefix.lowercased()
    guard !key.isEmpty, limit > 0 else { return [] }
    var best: [(word: String, frequency: UInt8)] = []
    var index = lowerBound(of: key)
    while index < keys.count, keys[index].hasPrefix(key) {
      if keys[index].count > key.count {
        best.append((words[index] ?? keys[index], frequencies[index]))
      }
      index += 1
    }
    return best.sorted { $0.frequency > $1.frequency }.prefix(limit).map(\.word)
  }

  /// The «ё» spelling to put in automatically, only when the word as typed is
  /// not a word in its own right.
  func yoSpelling(of word: String) -> String? {
    let key = word.lowercased()
    guard key.contains("е") else { return nil }
    return yoSpellings[key]
  }

  /// Every «ё» word that `word` could be, to offer in the suggestion bar.
  func yoVariants(of word: String) -> [String] {
    let key = word.lowercased()
    guard key.contains("е") else { return [] }
    var variants = yoWords[key] ?? []
    if let spelling = yoSpellings[key], !variants.contains(spelling) {
      variants.insert(spelling, at: 0)
    }
    return variants
  }

  private func lowerBound(of key: String) -> Int {
    var low = 0
    var high = keys.count
    while low < high {
      let middle = (low + high) / 2
      if keys[middle] < key { low = middle + 1 } else { high = middle }
    }
    return low
  }
}
