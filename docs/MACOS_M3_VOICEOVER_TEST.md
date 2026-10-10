# Preizkus z VoiceOverjem: enota M3 (prvo Mac okno)

To je prvi pravi ApricotPlayer na Macu, zaenkrat samo glavno okno. Vrstice
glavnega menija, ki še nimajo macOS zaslona, povedo, da v tej beti še niso na
voljo; Exit deluje. Podatki so v `~/Library/Application Support/ApricotPlayer2Beta`.

## Zagon

V Terminalu:

```
~/Downloads/ApricotPlayer/rust/scripts/run_macos_app.sh
```

Za preizkus izbire jezika ob prvem zagonu (začasen prazen profil, pravi
profil ostane nedotaknjen):

```
~/Downloads/ApricotPlayer/rust/scripts/run_macos_app.sh --first-run
```

Program zapreš z Command+Q ali z vrstico Exit.

## Kaj preveriti

Za vsako točko napiši, ali je bilo tako, in kaj je VoiceOver prebral, kadar
je bilo drugače. Primerjaj z Windows verzijo, kolikor se je spomniš.

1. **Izbira jezika** (samo z `--first-run`). Odpre se okno "Language" s
   seznamom jezikov, fokus je na seznamu, izbrana je angleščina. Puščice in
   začetnice delujejo. Enter (ali OK) izbere jezik, Escape (ali Cancel)
   ohrani angleščino. Po izbiri se odpre glavno okno v izbranem jeziku.
2. **Glavno okno.** Naslov okna je "ApricotPlayer 2 Beta 2.0.0-beta.1". Fokus
   je na seznamu "Main menu" (v slovenščini "Glavni meni"), izbrana je prva
   vrstica.
3. **Bližnjice v vrsticah.** Vrstice povedo bližnjico z imeni Command in
   Option, na primer "Search YouTube / SoundCloud  Command+Option+Y".
4. **Začetnice.** P, P, P kroži med vsemi vrsticami na P. "pl" hitro skupaj
   skoči na Playlists. Puščica vmes začne novo iskanje.
5. **Enter na vrstici.** Na primer na History: VoiceOver pove "History is not
   available in this beta yet." (v slovenščini ustrezno), fokus ostane na
   seznamu, izbrana vrstica se ne premakne.
6. **Gumb Open.** Tab gre s seznama na gumb Open, še en Tab nazaj na seznam.
   Shift+Tab v obratni smeri. Preslednica in Enter na Open storita isto kot
   Enter na seznamu.
7. **Globalna bližnjica.** Command+Option+Y pove, da iskanje še ni na voljo.
   Escape v glavnem meniju ne naredi ničesar.
8. **Exit.** Enter na vrstici Exit zapre program.
9. **Statusna vrstica.** Pod gumbom je besedilo stanja ("Ready." oziroma
   zadnje sporočilo); VoiceOver ga najde z navigacijo VO tipk, Tab ga
   preskoči, kot na Windows.
10. **Menijska vrstica.** Command+vejica (Settings) pove, da nastavitve še
    niso na voljo. Command+H skrije program, klik ali VO na ikoni v Docku
    ga vrne.
