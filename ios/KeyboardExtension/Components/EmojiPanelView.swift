import UIKit

/// Emoji in place of the key grid, opened by holding the microphone key: pages
/// scroll sideways as on Apple's emoji keyboard, a bar below switches
/// categories, returns to the letters and deletes.
final class EmojiPanelView: UIView, UICollectionViewDataSource, UICollectionViewDelegateFlowLayout {
  var onInsert: ((String) -> Void)?
  var onDelete: (() -> Void)?
  var onClose: (() -> Void)?

  private static let rowCount = 5
  private static let barHeight: CGFloat = 40

  private var sections: [EmojiCategory] = EmojiCatalog.categories
  private let titleLabel = UILabel()
  private let collectionView: UICollectionView
  private let bar = UIStackView()
  private var categoryButtons: [UIButton] = []
  private let lettersButton = UIButton(type: .system)
  private let deleteButton = UIButton(type: .system)
  private var deleteTimer: Timer?

  override init(frame: CGRect) {
    let layout = UICollectionViewFlowLayout()
    layout.scrollDirection = .horizontal
    layout.minimumInteritemSpacing = 0
    layout.minimumLineSpacing = 0
    collectionView = UICollectionView(frame: .zero, collectionViewLayout: layout)
    super.init(frame: frame)

    titleLabel.font = .systemFont(ofSize: 12, weight: .semibold)
    titleLabel.textColor = .secondaryLabel
    titleLabel.translatesAutoresizingMaskIntoConstraints = false

    collectionView.backgroundColor = .clear
    collectionView.showsHorizontalScrollIndicator = false
    collectionView.dataSource = self
    collectionView.delegate = self
    collectionView.register(EmojiCell.self, forCellWithReuseIdentifier: EmojiCell.reuseIdentifier)
    collectionView.translatesAutoresizingMaskIntoConstraints = false

    bar.axis = .horizontal
    bar.distribution = .fillEqually
    bar.alignment = .fill
    bar.translatesAutoresizingMaskIntoConstraints = false

    lettersButton.setTitle("АБВ", for: .normal)
    lettersButton.titleLabel?.font = .systemFont(ofSize: 15, weight: .medium)
    lettersButton.tintColor = .label
    lettersButton.accessibilityLabel = "Буквы"
    lettersButton.accessibilityIdentifier = "emoji-letters-button"
    lettersButton.addTarget(self, action: #selector(closeTapped), for: .touchUpInside)

    deleteButton.setImage(UIImage(systemName: "delete.backward"), for: .normal)
    deleteButton.tintColor = .label
    deleteButton.accessibilityLabel = "Удалить"
    deleteButton.addTarget(self, action: #selector(deleteDown), for: .touchDown)
    deleteButton.addTarget(
      self, action: #selector(deleteUp), for: [.touchUpInside, .touchUpOutside, .touchCancel])

    for subview in [titleLabel, collectionView, bar] { addSubview(subview) }
    NSLayoutConstraint.activate([
      titleLabel.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 14),
      titleLabel.topAnchor.constraint(equalTo: topAnchor),
      titleLabel.heightAnchor.constraint(equalToConstant: 16),
      collectionView.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
      collectionView.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
      collectionView.topAnchor.constraint(equalTo: titleLabel.bottomAnchor, constant: 2),
      collectionView.bottomAnchor.constraint(equalTo: bar.topAnchor),
      bar.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 4),
      bar.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -4),
      bar.bottomAnchor.constraint(equalTo: bottomAnchor),
      bar.heightAnchor.constraint(equalToConstant: Self.barHeight),
    ])
    accessibilityIdentifier = "emoji-panel"
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) {
    nil
  }

  deinit {
    deleteTimer?.invalidate()
  }

  /// Rebuilds the pages with the current recents in front.
  func reload(recents: [String]) {
    sections = EmojiCatalog.categories
    if !recents.isEmpty {
      sections.insert(
        EmojiCategory(title: EmojiCatalog.recentsTitle, symbol: "clock", emoji: recents), at: 0)
    }
    rebuildBar()
    collectionView.reloadData()
    collectionView.layoutIfNeeded()
    collectionView.setContentOffset(.zero, animated: false)
    selectCategory(0)
  }

  private func rebuildBar() {
    bar.arrangedSubviews.forEach { $0.removeFromSuperview() }
    categoryButtons = sections.enumerated().map { index, section in
      let button = UIButton(type: .system)
      button.setImage(
        UIImage(
          systemName: section.symbol, withConfiguration: UIImage.SymbolConfiguration(pointSize: 15)),
        for: .normal)
      button.tag = index
      button.accessibilityLabel = section.title
      button.addTarget(self, action: #selector(categoryTapped(_:)), for: .touchUpInside)
      return button
    }
    bar.addArrangedSubview(lettersButton)
    categoryButtons.forEach(bar.addArrangedSubview)
    bar.addArrangedSubview(deleteButton)
  }

  private func selectCategory(_ index: Int) {
    guard sections.indices.contains(index) else { return }
    titleLabel.text = sections[index].title.uppercased()
    for (buttonIndex, button) in categoryButtons.enumerated() {
      button.tintColor = buttonIndex == index ? .label : .secondaryLabel
    }
  }

  @objc private func categoryTapped(_ sender: UIButton) {
    let index = sender.tag
    guard sections.indices.contains(index), !sections[index].emoji.isEmpty else { return }
    collectionView.scrollToItem(
      at: IndexPath(item: 0, section: index), at: .left, animated: false)
    selectCategory(index)
  }

  @objc private func closeTapped() {
    stopDeleteRepeat()
    onClose?()
  }

  @objc private func deleteDown() {
    onDelete?()
    deleteTimer?.invalidate()
    let timer = Timer(timeInterval: 0.45, repeats: false) { [weak self] _ in
      self?.startDeleteRepeat()
    }
    deleteTimer = timer
    RunLoop.main.add(timer, forMode: .common)
  }

  private func startDeleteRepeat() {
    let timer = Timer(timeInterval: 0.09, repeats: true) { [weak self] _ in self?.onDelete?() }
    deleteTimer = timer
    RunLoop.main.add(timer, forMode: .common)
  }

  @objc private func deleteUp() {
    stopDeleteRepeat()
  }

  private func stopDeleteRepeat() {
    deleteTimer?.invalidate()
    deleteTimer = nil
  }

  // MARK: Collection

  func numberOfSections(in collectionView: UICollectionView) -> Int {
    sections.count
  }

  func collectionView(_ collectionView: UICollectionView, numberOfItemsInSection section: Int) -> Int {
    sections[section].emoji.count
  }

  func collectionView(
    _ collectionView: UICollectionView, cellForItemAt indexPath: IndexPath
  ) -> UICollectionViewCell {
    let cell = collectionView.dequeueReusableCell(
      withReuseIdentifier: EmojiCell.reuseIdentifier, for: indexPath)
    (cell as? EmojiCell)?.emoji = sections[indexPath.section].emoji[indexPath.item]
    return cell
  }

  func collectionView(
    _ collectionView: UICollectionView, layout collectionViewLayout: UICollectionViewLayout,
    sizeForItemAt indexPath: IndexPath
  ) -> CGSize {
    let side = floor(collectionView.bounds.height / CGFloat(Self.rowCount))
    return CGSize(width: max(side, 1), height: max(side, 1))
  }

  func collectionView(
    _ collectionView: UICollectionView, layout collectionViewLayout: UICollectionViewLayout,
    insetForSectionAt section: Int
  ) -> UIEdgeInsets {
    UIEdgeInsets(top: 0, left: 0, bottom: 0, right: 12)
  }

  func collectionView(_ collectionView: UICollectionView, didSelectItemAt indexPath: IndexPath) {
    collectionView.deselectItem(at: indexPath, animated: true)
    onInsert?(sections[indexPath.section].emoji[indexPath.item])
  }

  func scrollViewDidScroll(_ scrollView: UIScrollView) {
    let probe = CGPoint(x: scrollView.contentOffset.x + 8, y: scrollView.bounds.midY)
    if let indexPath = collectionView.indexPathForItem(at: probe) {
      selectCategory(indexPath.section)
    }
  }
}

private final class EmojiCell: UICollectionViewCell {
  static let reuseIdentifier = "emoji"
  private let label = UILabel()

  var emoji: String = "" {
    didSet {
      label.text = emoji
      accessibilityLabel = emoji
    }
  }

  override init(frame: CGRect) {
    super.init(frame: frame)
    label.font = .systemFont(ofSize: 30)
    label.textAlignment = .center
    label.adjustsFontSizeToFitWidth = true
    label.minimumScaleFactor = 0.6
    label.translatesAutoresizingMaskIntoConstraints = false
    contentView.addSubview(label)
    NSLayoutConstraint.activate([
      label.leadingAnchor.constraint(equalTo: contentView.leadingAnchor),
      label.trailingAnchor.constraint(equalTo: contentView.trailingAnchor),
      label.topAnchor.constraint(equalTo: contentView.topAnchor),
      label.bottomAnchor.constraint(equalTo: contentView.bottomAnchor),
    ])
    isAccessibilityElement = true
    accessibilityTraits = .keyboardKey
    let highlight = UIView()
    highlight.backgroundColor = UIColor.label.withAlphaComponent(0.12)
    highlight.layer.cornerRadius = 8
    selectedBackgroundView = highlight
  }

  @available(*, unavailable)
  required init?(coder: NSCoder) {
    nil
  }
}
