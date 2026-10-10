# Preizkus z VoiceOverjem: enota M2 (AppKit spike)

To je majhno preizkusno okno, ne pravi ApricotPlayer. Preverja, ali so
macOS kontrole, ki jih bo uporabljal pravi program, z VoiceOverjem in
tipkovnico take, kot zahteva pogodba.

## Zagon

1. Odpri Terminal (Command+preslednica, napiši Terminal, Enter).
2. Prilepi ta ukaz in pritisni Enter:

   ```
   ~/Downloads/ApricotPlayer/rust/scripts/run_macos_spike.sh
   ```

3. Odpre se okno "ApricotPlayer accessibility spike". Fokus je na seznamu.
4. Vklopi VoiceOver, če še ni (Command+F5).

Okno zapreš s Command+Q.

## Kaj preveriti

Za vsako točko mi napiši, ali je bilo tako, in kaj je VoiceOver dejansko
prebral, če je bilo drugače.

1. **Seznam.** Ob zagonu VoiceOver prebere seznam z imenom "Main menu" in
   izbrano vrstico "Search YouTube / SoundCloud".
2. **Puščice.** Puščica dol in gor premikata po vrsticah, VoiceOver prebere
   vsako vrstico enkrat.
3. **Začetnice.** Pritisk na P skoči na "Play file", ponovni P na
   "Playlists" ali "Podcasts and RSS". S na "Settings", H na "History".
4. **Enter.** Enter na vrstici odpre okno "You opened ...". VoiceOver ga
   prebere. Enter ali preslednica ga zapre in fokus se vrne na isto vrstico
   seznama, brez dodatnega branja česa drugega.
5. **Kontekstni meni s Shift+F10.** Na MacBooku je to Fn+Shift+F10. Odpre se
   meni Play, Add to favorites, Copy link. Izbira postavke prebere
   "Context menu: ..." in fokus se vrne na seznam. Escape zapre meni.
6. **Kontekstni meni z VoiceOverjem.** VO+Shift+M (VO je Control+Option) na
   seznamu odpre isti meni.
7. **Tab.** Iz seznama Tab po vrsti: Open, Announce, Play test video, polje
   Search, Provider, Autoplay next, Volume, nazaj na seznam. Shift+Tab gre v
   obratni smeri. Vsaka kontrola ima pravo ime in vlogo (gumb, besedilno
   polje, pojavni meni, potrditveno polje, drsnik).
8. **Gumbi.** Preslednica in Enter na gumbu Announce izgovorita "Announcement
   test. This text was spoken without moving focus." Fokus ostane na gumbu.
9. **Potrditveno polje.** Preslednica na "Autoplay next" ga obkljuka in
   VoiceOver prebere novo stanje.
10. **Drsnik.** Na "Volume" puščici levo in desno spremenita vrednost,
    VoiceOver bere na primer "51 percent".
11. **Pojavni meni.** Na "Provider" puščice ali preslednica izberejo YouTube
    ali SoundCloud.
12. **Video.** Na "Play test video" pritisni preslednico. Zasliši se ton 440
    Hz, ki po minuti preneha, in fokus ostane na gumbu. V oknu levo spodaj
    naj bi tekel barvni preizkusni video, v istem oknu in ne v novem. Da
    libmpv slike riše v to okno, sem preveril samodejno (okoli 30 slik na
    sekundo, nobenega dodatnega okna); če vidiš ali ti lahko kdo pogleda,
    potrdi še na zaslonu.
13. **Escape.** Escape na seznamu izgovori "Escape pressed."
