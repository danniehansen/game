# UI And Client Flow

`src/app.rs` wires the Bevy app. `src/app/ui` draws menus, worlds, HUD, pause, chat, confirmation, and multiplayer views.

Screens live in `MenuState`: `MainMenu`, `Worlds`, `Multiplayer`, `InGame`. Multiplayer UI exists, but the main-menu entry is gated as coming soon.

Input systems:
- Enter/T opens chat.
- Escape toggles pause.
- In-game cursor capture drives mouse look.
- WASD, shift, and space feed predicted movement.

Scene rendering uses a first-person camera, generated floor/block geometry, and replicated player capsules.

Audio:
- `assets/main-screen/ambient-music.wav` loops across main-menu, worlds, and multiplayer menu screens.
- Main-menu ambience is managed by `main_menu_music_system` and fades out when the user loads into a world.
- Runtime audio should stay WAV unless there is a specific reason to add another decoder feature; earlier MP3/OGG experiments exposed decoder and seek reliability problems.

UI audio:
- Button click and hover sounds live at `assets/ui/button-click.wav` and `assets/ui/button-hover.wav`.
- `theme::game_button` and `theme::compact_button` record button sound requests while drawing egui widgets.
- Click sounds fire from `Response::clicked()`.
- Hover sounds fire only on hover entry, not every hovered frame.
- `button_sound_system` uses preloaded handles and spawns `PlaybackSettings::DESPAWN` one-shots, so rapid hover/click events can overlap without reusing a paused audio timeline.
- Keep hover SFX subtle and trimmed to the audible transient. Perceptual delay is very noticeable on hover, even when the scheduler is correct.
