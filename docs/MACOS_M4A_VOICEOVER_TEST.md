# Preizkus z VoiceOverjem: enota M4a (predvajalnik za lokalne datoteke)

Primerjaj z Windows verzijo, kolikor se je spomniš. Za vsako točko napiši,
ali je bilo tako, in kaj je VoiceOver prebral, kadar je bilo drugače.

## Zagon

Če program že teče, ga zapri s Command+Q. Nato v Terminalu:

```
~/Downloads/ApricotPlayer/rust/scripts/run_macos_app.sh
```

Pripravi eno zvočno datoteko (na primer mp3) in en video (na primer mp4).

## Kaj preveriti

1. **Play file.** V glavnem meniju Enter na "Play file" (ali Command+Option+I)
   odpre okno za izbiro datoteke. Izberi zvočno datoteko. Odpre se stran
   predvajalnika, fokus je na "Player", VoiceOver pove "Playing: ime".
   Naslov okna je ime datoteke.
2. **Escape v izbirnem oknu.** Če okno za izbiro zapreš z Escape, ostaneš v
   glavnem meniju na isti vrstici.
3. **Tipke na "Player".** Preslednica premor in nadaljevanje (pove "Paused."
   in "Playing."), T čas, V glasnost, F format, puščici gor in dol glasnost
   brez oglasa, levo in desno premik, Command+Home in Command+End začetek in
   konec, S in D hitrost, Command+puščici gor in dol višina tona, Command+0
   ponastavitev s kratkim zvokom, R ponavljanje.
4. **Držanje tipk.** Držiš desno puščico: premika se enakomerno in neha takoj,
   ko spustiš. Enako za D in S.
5. **Tab.** Tab gre s "Player" na gumbe po vrsti (Previous, Pause, Next,
   ...) in na potrditvena polja (Full screen, Repeat, ...); Shift+Tab nazaj.
   Imena gumbov vsebujejo bližnjice s Command in Option.
6. **Gumbi.** Preslednica ali Enter na gumbu Pause ustavi predvajanje; ob
   tem VoiceOver ne pove še "Paused." posebej, ker je fokus na tem gumbu.
   Potrditveno polje Repeat s preslednico pove "Repeat on." in ostane
   obkljukano.
7. **Gumbi, ki še niso narejeni** (izhodne naprave, izenačevalnik,
   poglavja, ...), povedo, da v tej beti še niso na voljo.
8. **Konec posnetka.** Ko se datoteka izteče, VoiceOver pove "Playback
   finished." (če je oglaševanje vklopljeno v nastavitvah).
9. **Escape.** Escape na "Player" ali na gumbu ustavi predvajanje in vrne v
   glavni meni; naslov okna je spet "ApricotPlayer 2 Beta".
10. **Video.** Odpri video datoteko. Zvok teče, video je v istem oknu (če
    vidiš ali ti lahko kdo pogleda).
11. **Odpiranje iz Finderja ali drugi zagon.** Med tekom programa v Terminalu
    poženi `~/Downloads/ApricotPlayer/rust/target/debug/apricot-player
    "pot/do/datoteke.mp3"`. Datoteka se predvaja v že odprtem oknu, novo okno
    se ne odpre.
12. **Nadaljevanje pozicije.** Predvajaj dlje časa, Escape, nato isto
    datoteko odpri znova: nadaljuje, kjer si končal (kot na Windows).
