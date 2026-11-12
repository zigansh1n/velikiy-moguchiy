import UIKit
import XCTest

final class WordFrequencyListTests: XCTestCase {
  private let list = WordFrequencyList(
    wordsTSV: "привет\t150\nпривёз\t100\nпривычка\t120\nвсе\t168\nвсё\t153\nещё\t162\nМосква\t140\n",
    yoTSV: "еще\tещё\nпривез\tпривёз\n")

  func testCompletionsAreTheCommonestLongerWords() {
    XCTAssertEqual(list.completions(for: "При", limit: 2), ["привет", "привычка"])
    XCTAssertEqual(list.completions(for: "мос", limit: 3), ["Москва"])
    XCTAssertEqual(list.completions(for: "привет", limit: 3), [])
  }

  func testLookupIgnoresCase() {
    XCTAssertTrue(list.contains("Привет"))
    XCTAssertTrue(list.contains("москва"))
    XCTAssertFalse(list.contains("превет"))
  }

  func testYoIsPutInOnlyWhereTheEWordDoesNotExist() {
    XCTAssertEqual(list.yoSpelling(of: "Еще"), "ещё")
    XCTAssertNil(list.yoSpelling(of: "все"), "все is a word of its own")
    XCTAssertEqual(list.yoVariants(of: "все"), ["всё"])
    XCTAssertEqual(list.yoVariants(of: "еще"), ["ещё"])
    XCTAssertEqual(list.yoVariants(of: "дом"), [])
  }

  func testTheBundledListLoadsWhenItShips() throws {
    let url = URL(fileURLWithPath: #filePath)
      .deletingLastPathComponent().deletingLastPathComponent()
      .appendingPathComponent("Resources/Keyboard")
    let words = try String(contentsOf: url.appendingPathComponent("ru-words.tsv"), encoding: .utf8)
    let yo = try String(contentsOf: url.appendingPathComponent("ru-yo.tsv"), encoding: .utf8)
    let list = WordFrequencyList(wordsTSV: words, yoTSV: yo)
    XCTAssertGreaterThan(list.count, 40_000)
    XCTAssertEqual(list.yoSpelling(of: "еще"), "ещё")
    XCTAssertEqual(list.yoSpelling(of: "ее"), "её")
    XCTAssertNil(list.yoSpelling(of: "все"))
    XCTAssertTrue(list.yoVariants(of: "все").contains("всё"))
    XCTAssertEqual(list.completions(for: "спас", limit: 1), ["спасибо"])
  }
}

final class RussianSuggestionEngineTests: XCTestCase {
  private func makeEngine() -> SuggestionEngine {
    let list = WordFrequencyList(
      wordsTSV: "привет\t150\nпривычка\t120\nвсе\t168\nвсё\t153\nещё\t162\n",
      yoTSV: "еще\tещё\n")
    let engine = SuggestionEngine(language: .russian, checker: nil, wordList: { list })
    engine.queue.sync {}
    return engine
  }

  func testCompletionsComeFromTheWordListWithoutASystemDictionary() {
    let engine = makeEngine()
    XCTAssertEqual(engine.suggestions(contextBefore: "Ну при").map(\.text), ["при", "привет", "привычка"])
  }

  func testTheYoSpellingLeadsTheSuggestions() {
    let engine = makeEngine()
    XCTAssertEqual(engine.suggestions(contextBefore: "это все").map(\.text).prefix(2), ["все", "всё"])
    XCTAssertEqual(engine.suggestions(contextBefore: "Еще").map(\.text).prefix(2), ["Еще", "Ещё"])
  }

  func testYoOnSpaceOnlyForUnambiguousWordsAndNotAfterUndo() {
    let engine = makeEngine()
    XCTAssertEqual(
      engine.replacement(contextBefore: "Еще", autocorrectEnabled: false, yoEnabled: true),
      BoundaryReplacement(original: "Еще", replacement: "Ещё", isAutocorrect: true))
    XCTAssertNil(engine.replacement(contextBefore: "все", autocorrectEnabled: false, yoEnabled: true))
    XCTAssertNil(engine.replacement(contextBefore: "еще", autocorrectEnabled: false, yoEnabled: false))
    engine.reject("еще")
    XCTAssertNil(engine.replacement(contextBefore: "еще", autocorrectEnabled: false, yoEnabled: true))
  }
}

final class KeyboardExtrasTests: XCTestCase {
  func testHoldingAFunctionKeySuppressesItsTap() {
    var tracker = KeyTouchTracker()
    let mic = KeyTarget(id: KeyID(row: 3, column: 1), frame: CGRect(x: 0, y: 0, width: 40, height: 40), kind: .function)
    tracker.targets = [mic]
    _ = tracker.began(touch: 1, at: CGPoint(x: 20, y: 20))
    XCTAssertEqual(
      tracker.holdFunction(touch: 1),
      [.released(touch: 1, key: mic.id), .held(touch: 1, key: mic.id)])
    XCTAssertEqual(tracker.ended(touch: 1, at: CGPoint(x: 20, y: 20)), [.released(touch: 1, key: mic.id)])
  }

  func testTheNumberRowSitsAboveTheLettersAndKeepsTheHeightOnEveryPage() {
    var state = KeyboardInteractionState(language: .russian)
    let geometry = KeyboardGeometry.portrait
    let plain = state.layout(inputKind: .standard, needsInputModeSwitchKey: false)
    XCTAssertEqual(plain.count, 4)
    XCTAssertEqual(geometry.rowHeights(for: plain, gridHeight: geometry.gridHeight), [43, 43, 43, 43])

    state.showsNumberRow = true
    let letters = state.layout(inputKind: .standard, needsInputModeSwitchKey: false)
    XCTAssertEqual(letters.count, 5)
    XCTAssertEqual(letters[0].keys.map(\.action), "1234567890".map { .text(String($0)) })
    let total = geometry.gridHeightWithNumberRow
    let letterHeights = geometry.rowHeights(for: letters, gridHeight: total)
    XCTAssertEqual(letterHeights[1], 43, accuracy: 0.001, "letter keys keep Apple's height")
    XCTAssertLessThan(letterHeights[0], 43)

    state.tapPage()
    let numbers = state.layout(inputKind: .standard, needsInputModeSwitchKey: false)
    XCTAssertEqual(numbers.count, 4)
    let numberHeights = geometry.rowHeights(for: numbers, gridHeight: total)
    XCTAssertEqual(numberHeights.reduce(0, +) + geometry.rowGap * 3, total, accuracy: 0.001)

    XCTAssertEqual(state.layout(inputKind: .numberPad, needsInputModeSwitchKey: false).count, 4)
  }

  func testRecentEmojiAreNewestFirstWithoutDuplicates() {
    var recents: [String] = []
    for emoji in ["😀", "👍", "😀"] { recents = EmojiRecents.adding(emoji, to: recents) }
    XCTAssertEqual(recents, ["😀", "👍"])
    for index in 0..<100 { recents = EmojiRecents.adding("\(index)", to: recents) }
    XCTAssertEqual(recents.count, EmojiRecents.limit)
  }

  func testEveryCategoryHasEmojiAndNoDuplicates() {
    for category in EmojiCatalog.categories {
      XCTAssertFalse(category.emoji.isEmpty, category.title)
      XCTAssertEqual(Set(category.emoji).count, category.emoji.count, category.title)
      XCTAssertNotNil(UIImage(systemName: category.symbol), category.symbol)
    }
  }
}
