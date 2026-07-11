# Potrjeni backlog

Ta datoteka vsebuje samo potrjeno prihodnje delo. Stare ideje iz chatov niso avtomatsko del backloga.

## Pred stabilno 1.0

### AudioVault integracija

Status: prva celovita integracija je dodana v `1.0.0-beta.48`; sledi preverjanje z dejanskim računom in stabilizacija skozi beta izdaje.

AudioVault bo pomemben feature in mora biti najprej dodan ter stabiliziran skozi beta izdaje. Pred implementacijo je treba z uporabnikom natančno določiti:

- katere AudioVault vsebine in dejanja mora Apricot podpirati;
- način prijave oziroma avtorizacije;
- iskanje, brskanje, metapodatke in strukturo rezultatov;
- predvajanje, queue, history, favorites in morebitne prenose;
- keyboard shortcute, kontekstne menije in Action Finder akcije;
- NVDA imena, focus flow, loading in error obvestila;
- cache, offline vedenje, rate limite in varno shranjevanje poverilnic;
- minimalni prvi beta obseg ter kriterije za pripravljenost stabilne 1.0.

Implementacije ne začni z ugibanjem o AudioVault API-ju. Najprej pripravi raziskavo trenutnih uradnih možnosti in predlog obsega, nato ga potrdi z uporabnikom.

## Že zaključeno, ne backlog

`docs/ROADMAP_1.0.md` je izveden. Med drugim so že dodani diagnostic report, transcripts, bookmarks, normalization, EQ Profiles 2.0, Action Finder, session restore, Comments 2.0, podcast mode ter stable/beta channel izboljšave.

Chapters, lyrics, SoundCloud, YouTube Shorts, subscription/RSS kategorije in dostopno branje posameznih medijskih polj so prav tako že v trenutni kodi.
