Flashcards

Matcha's Flashcards as a plugin, written in Rust: your Anki and Migaku decks,
synced through matcha-anki-server, reviewed with FSRS-5 and Anki's learning
steps, burying, Easy Days and leeches, as matcha does. Reviews go back to Anki
(or Migaku) with each sync.

Proof of concept: cards show as plain text and 1-bit pictures, without the
deck's styling or furigana. Pictures come as PNG; other formats need
matcha-anki-server with PNG copies (its dessen branch).

Setup: put a config.json in this folder:
  {"server": "http://192.168.1.2:8080", "key": "your server key",
   "anki": ["Kaishi 1.5k"], "migaku": ["日本語"]}
then open Flashcards and choose Sync Flashcards.

Insights and Flashcards share matcha's tab bar at the bottom: Home, Library,
Transfer, Insights, Flashcards and Settings. Tap a tab, or press Down past the
last row to move into the bar, Up/Down to pick a tab and Confirm to switch.
Home closes the plugin (dessen has no call to open Home itself).
