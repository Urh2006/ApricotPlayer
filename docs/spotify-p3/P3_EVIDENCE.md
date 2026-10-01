# P3 dokazi: kontekst, vrsta, shuffle, repeat

Preizkus 1. 10. 2026 na ločenem nevidnem namizju, kopija podatkov s testno
prijavo (Premium), glasnost 0, razvojna gradnja `rust-2.0`. Gonilnik pošilja
tipke v okna aplikacije in bere naslov okna, statusno vrstico, fokus in
vsebino seznamov. Žetoni in poverilnice niso v izpisu.

## Kontekst, Next/Previous, shuffle, repeat, gapless

Direct link `spotify:album:4aawyAB9vmqN3uQ7FjRGTy`:

```
album started -> title 'Global Warming (feat. Sensato)'; Elapsed 0:04, remaining 1:20, total 1:25.
after Ctrl+PageDown and 4 s -> title 'Don't Stop the Party (feat. TJR)'; Elapsed 0:03, remaining 3:22, total 3:26.
after Ctrl+PageUp (po več kot 3 s začne isto skladbo znova) -> title 'Don't Stop the Party (feat. TJR)'; Elapsed 0:03, total 3:26.
Shift+S: Shuffle on.   Shift+S: Shuffle off.
R: Repeat album or playlist.   R: Repeat track.   R: Repeat off.
6 s after Ctrl+End -> title 'Feel This Moment (feat. Christina Aguilera)'; Elapsed 0:05, remaining 3:44, total 3:49.
```

Zadnja vrstica je prehod brez vrzeli: v dnevniku `gapless boundary at 482 ms of
the stream`, brez novega `loadfile`.

## Spotify vrsta (Ctrl+Alt+Shift+Q)

```
Ctrl+Shift+Q: Added to Spotify queue.  (dvakrat, predvajalnik)
opened -> count 18, selected 1: Now playing: Feel This Moment ... // Feel This Moment ..., added manually // Feel This Moment ..., added manually // Back in Time ..., next from the album or playlist // ...
after Move down on the first added track -> count 18, selected 2 (ista pojavitev)
after Delete -> count 17, selected 2
after Clear -> count 16 (ročno dodanih ni več, kontekst ostane)
Enter on 'Hope We Meet Again ...' -> dialog zaprt; title 'Hope We Meet Again (feat. Chris Brown)'; Playing: Hope We Meet Again (feat. Chris Brown); focus Static 'Player'
reopened -> Now playing: Hope We Meet Again ... // Party Ain't Over ..., next from the album or playlist ...
after Escape: dialog zaprt; focus Static 'Player'
```

## Izhod med predvajanjem

Pred popravkom: `playing : still running 15 s after WM_CLOSE` (velja tudi za
beta.5). Vzrok: nit LibreSpot predvajalnika je čakala na prostor v obroču, ki
ga po zaprtju mpv nihče ne bere, izhod pa je čakal na to nit. Po popravku
(`Shared::close_all` ob koncu seje, test
`closing_the_session_releases_a_writer_waiting_for_ring_space`):

```
playing : exited 218 ms after WM_CLOSE
paused : exited 206 ms after WM_CLOSE
idle : exited 210 ms after WM_CLOSE
```
