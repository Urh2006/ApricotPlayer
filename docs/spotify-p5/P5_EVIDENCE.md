# P5 dokazi: Domov, Daily Mixes, radio, nedavno predvajano, brskanje

Preizkus 1. 10. 2026 na ločenem nevidnem namizju s kopijo podatkov, samo branje.

```
Home -> 20 razdelkov: Made For Urh.strakl, section, 8 items // Your top mixes, section, 10 items // Recently played ... // Recommended Stations ...
section -> Daily Mix 1, playlist, Spotify // Daily Mix 2 ... (8)
Daily Mixes (Ctrl+Alt+Shift+M) -> Daily Mix 1 ... Daily Mix 6
Daily Mix 1 -> 50 skladb z oznako liked
Enter on the fifth mix track 'DOOMSDAY 4EVER, ...': title 'DOOMSDAY 4EVER'; Playing: DOOMSDAY 4EVER
back from the player -> selected 4 (ista vrstica)
Ctrl+Alt+Shift+R -> 'DOOMSDAY 4EVER Radio, playlist, Spotify', 50 skladb
Recently played -> On Repeat // Global Warming, album, Pitbull // Liked Songs // Ixper Radio ...
Browse categories -> Browse all, section, 59 items -> Music, category // Podcasts // Fitness ...
Music -> Discover new music, section, 3 items // Playlists from our Editors, 37 items ...
Your top tracks and artists -> Top tracks, last 4 weeks, 50 // Top artists, last 4 weeks, 20 // ... all time
Top tracks, last 4 weeks -> You Are My Storm, Solstice, 4:06 // My Heart On You, Solstice, 2:59, liked ...
```

Najdeno med preizkusom: osebni mix (Daily Mix) Spotify ob vsaki zahtevi
sestavi znova, zato pathfinder `uid` ni v Connectovi kopiji in se je začela
prva skladba. Popravek: v osebnih miksih po URI. `userTopContent` sprejme
obdobja `SHORT_TERM`, `MID_TERM`, `LONG_TERM` (ne `MEDIUM_TERM`).
