import UIKit

extension KeyboardViewController {
  /// The panel covers the keys; it never opens during a dictation, whose result
  /// still has to land where the cursor was.
  func showEmojiPanel() {
    guard mode == .idle, emojiPanel.isHidden else { return }
    keyboardSurface.resetTransientState()
    emojiPanel.reload(recents: emojiRecents)
    emojiPanel.isHidden = false
    keyboardSurface.isHidden = true
    UIAccessibility.post(notification: .screenChanged, argument: emojiPanel)
  }

  func hideEmojiPanel() {
    guard !emojiPanel.isHidden else { return }
    emojiPanel.isHidden = true
    keyboardSurface.isHidden = false
    UIAccessibility.post(notification: .screenChanged, argument: keyboardSurface)
  }

  func insertEmoji(_ emoji: String) {
    typeText(emoji)
    emojiRecents = EmojiRecents.adding(emoji, to: emojiRecents)
  }

  /// Per keyboard and kept in the extension's own defaults: no Full Access needed.
  var emojiRecents: [String] {
    get { UserDefaults.standard.stringArray(forKey: EmojiRecents.defaultsKey) ?? [] }
    set { UserDefaults.standard.set(newValue, forKey: EmojiRecents.defaultsKey) }
  }
}
