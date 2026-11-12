import Foundation

/// Keyboard behaviour switches the app writes and the keyboards read.
enum KeyboardPreferences {
  static let autocorrectKey = "configuration.autocorrect-enabled"

  /// Automatic correction on space is OFF unless the user turned it on: a wrong
  /// silent replacement is worse than a missing one.
  static func isAutocorrectEnabled(_ stored: Any?) -> Bool {
    (stored as? NSNumber)?.boolValue ?? false
  }

  static let yoKey = "configuration.yo-enabled"

  /// «ё» on space (еще -> ещё) is ON: it touches only words that do not exist
  /// with «е», and ⌫ puts the typed word back.
  static func isYoEnabled(_ stored: Any?) -> Bool {
    (stored as? NSNumber)?.boolValue ?? true
  }

  static let numberRowKey = "configuration.number-row-enabled"

  /// The digit row is OFF, so the keyboard stays the size of Apple's.
  static func isNumberRowEnabled(_ stored: Any?) -> Bool {
    (stored as? NSNumber)?.boolValue ?? false
  }
}
