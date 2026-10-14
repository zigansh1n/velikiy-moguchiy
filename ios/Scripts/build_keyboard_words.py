#!/usr/bin/env python3
"""Builds the Russian keyboard's word list from the Android one.

usage: build_keyboard_words.py   (run from anywhere; reads and writes inside the repo)

Input:  android/app/src/main/assets/dictionary/ru.tsv (AOSP LatinIME words and
        frequencies, already stripped of personal names; see android/THIRD_PARTY.md).
Output: ios/Resources/Keyboard/ru-words.tsv  word<TAB>frequency, the words common
        enough for completions;
        ios/Resources/Keyboard/ru-yo.tsv     е-spelling<TAB>ё-spelling, only where the
        е-spelling is not a word of its own anywhere in the full list (еще -> ещё,
        but never все -> всё).
"""
import os

MIN_FREQUENCY = 90
# The source counts encyclopedia text, where «спасти» beats «спасибо». Everyday
# words of messages get a frequency above every source word so they lead.
CONVERSATIONAL_FREQUENCY = 200
CONVERSATIONAL = """
спасибо пожалуйста привет здравствуйте пока давай давайте хорошо ладно конечно наверное
сейчас сегодня завтра вчера потом скоро уже ещё тоже только просто очень спокойной ночи
доброе утро добрый вечер день как дела нормально отлично понятно согласен согласна извини
извините прости простите можно нужно надо хочу хочешь могу можешь будешь буду знаю знаешь
думаю кажется вообще кстати короче вроде точно правда серьёзно классно круто супер ага угу
напиши напишу позвони позвоню перезвоню созвонимся встретимся увидимся приеду приду еду иду
дома работе работаю занят занята освобожусь минут минуту час часа скинь скину отправь отправлю
посмотри посмотрю спроси спрошу скажи скажу люблю целую обнимаю жду ждёшь получилось получил
получила сделал сделала сделаю купить купи куплю забери заберу встреча встречу вечером утром
днём сегодняшний неделе выходные понедельник вторник среду четверг пятницу субботу воскресенье
""".split()

root = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
source = os.path.join(root, "android/app/src/main/assets/dictionary/ru.tsv")
out_dir = os.path.join(root, "ios/Resources/Keyboard")

rows = []
with open(source, encoding="utf-8") as f:
    for line in f:
        word, frequency = line.rstrip("\n").split("\t")
        rows.append((word, int(frequency)))

known = {word.lower() for word, _ in rows}
frequency_of = {w: f for w, f in rows if f >= MIN_FREQUENCY}
for word in CONVERSATIONAL:
    frequency_of[word] = CONVERSATIONAL_FREQUENCY
    known.add(word)
common = sorted(frequency_of.items(), key=lambda r: (-r[1], r[0]))

yo = {}
for word, _ in rows:
    lower = word.lower()
    if "ё" not in lower:
        continue
    plain = lower.replace("ё", "е")
    if plain in known:
        continue
    yo.setdefault(plain, set()).add(lower)

os.makedirs(out_dir, exist_ok=True)
with open(os.path.join(out_dir, "ru-words.tsv"), "w", encoding="utf-8") as f:
    for word, frequency in common:
        f.write(f"{word}\t{frequency}\n")
with open(os.path.join(out_dir, "ru-yo.tsv"), "w", encoding="utf-8") as f:
    for plain in sorted(yo):
        if len(yo[plain]) == 1:
            f.write(f"{plain}\t{next(iter(yo[plain]))}\n")
print(f"{len(common)} words, {sum(1 for v in yo.values() if len(v) == 1)} ё-spellings")
