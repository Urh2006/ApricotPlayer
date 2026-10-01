# P4 dokazi: iskanje, knjižnica, seznami, urejanje

Preizkus 1. 10. 2026 na ločenem nevidnem namizju s kopijo podatkov in testno
prijavo, glasnost 0. Samo branje; nobena sprememba Urhovega računa.

## Adapter

Pathfinder persisted queries se preberejo iz namiznega spletnega
predvajalnika (brez namiznega imena brskalnika Spotify vrne mobilni
predvajalnik) in iz njegovih lenih delov: 183 operacij, med njimi
`searchDesktop`, `searchTracks` ... `searchAudiobooks`, `libraryV3`,
`fetchLibraryTracks`, `getAlbum`, `fetchPlaylist`, `queryArtistOverview`,
`queryPodcastEpisodes`, `areEntitiesInLibrary`, `addToLibrary`,
`removeFromLibrary`, `addToPlaylist`, `removeFromPlaylist`,
`moveItemsInPlaylist`. Hashi so shranjeni v `spotify/pathfinder.json` in se ob
neznani operaciji preberejo znova.

## Seznami (izpis gonilnika)

```
results (All) -> count 60: Feel This Moment (feat. Christina Aguilera), track, Pitbull, Christina Aguilera, 3:49 // ...
artist (row 10) -> count 53: On The Floor, Jennifer Lopez, Pitbull, 4:44 // ...; focus ListBox 'Pitbull, artist'
album (row 18) -> count 18: Global Warming (feat. Sensato), Pitbull, Sensato, 1:25 // ...; focus ListBox 'Global Warming, album, Pitbull'
Enter on 'Feel This Moment ...': title 'Feel This Moment (feat. Christina Aguilera)'; Playing: Feel This Moment (feat. Christina Aguilera)
queue: Now playing: Feel This Moment ... // Back in Time ..., next from the album or playlist
after Escape (album -> artist) -> focus ListBox 'Global Warming, album, Pitbull'
after Escape (artist -> results) -> selected 18; focus ListBox 'Pitbull, artist'
library -> count 50: Liked Songs, playlist, 13353 songs // On Repeat, playlist, Spotify // ... // Solstice, artist
Liked Songs -> count 50; after End (next page) -> count 100, selected 49
search with liked state -> Dragostea din tei (The Saints Remix), track, O-Zone, The Saints, 2:39, liked // ...
own playlist -> Love Takes Over, Maddix, Sarah de Warren, 2:57, liked // ...
Enter on 'Acid Soul, Maddix, 4:08, liked' (4. vrstica): title 'Acid Soul'; queue: Now playing: Acid Soul, Maddix
Ctrl+Shift+H in Liked Songs: Spotify offers Hide song only in personal mixes such as Daily Mix.
On Repeat -> 30 vrstic, set skritih skladb prebran (0 skritih)
exited 209 ms after WM_CLOSE
```

Prva različica je skladbo v albumu začela po pathfinder `uid`, ki ga Connect
ne pozna, zato je Spotify začel prvo skladbo, Apricot pa je kazal izbrano.
Popravek: v albumu, oddaji in pri izvajalcu po URI, v playlistu po `uid`;
poleg tega Apricot ob začetku drugačne skladbe zamenja postavko.

## Spremembe računa (z Urhovim dovoljenjem, 1. 10. 2026)

```
status: Playlist created: ApricotPlayer P4 test B.
add 1, 2, 3 (tretja je dvojnik prve): Added to ApricotPlayer P4 test B.
like 1: Added to Liked Songs.   row: Bohemian Rhapsody, Queen, 5:55, liked
like 2: Removed from Liked Songs.   row: Bohemian Rhapsody, Queen, 5:55
test playlist -> 5:55 // 5:54 // 5:55
move down: Playlist updated.   -> 5:54 // 5:55 // 5:55, selected 1
delete row 3: Removed from this playlist.   -> 5:54 // 5:55
reopened test playlist (confirmed by Spotify) -> 5:54 // 5:55
status: Playlist renamed: ApricotPlayer P4 test B 2.
hide 'You Are My Storm, Solstice, 4:06': Song hidden.   row: ..., hidden
show again: Song shown again.
remove 'ApricotPlayer P4 test B 2*' from library: Removed from your library.
remove 'ApricotPlayer P4 test 2*' from library: Removed from your library.
test playlists still listed: False
```

Najdeni in popravljeni napaki: `addToPlaylist` potrebuje `playlistItemUris`
(Spotify je vrnil 400), playlist pa v knjižnico ne gre prek `addToLibrary`
(Spotify zavrne tip), ampak prek rootlist z `baseRevision` in točnim indeksom.
Po preizkusu je stanje računa enako kot prej: rootlist 136 vnosov (prej 136,
med preizkusom 138), Všečkane skladbe 13353 z istimi prvimi skladbami, skrita
skladba spet prikazana.
