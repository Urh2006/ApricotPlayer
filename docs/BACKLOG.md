# Potrjeni backlog

Ta datoteka vsebuje samo potrjeno prihodnje delo. Stare ideje iz chatov niso avtomatsko del backloga.

## ApricotPlayer 2.0 Rust rewrite

Status: potrjen in arhitekturno splaniran; implementacija se začne na uporabnikov
ukaz `začnimo`.

Celotna ApricotPlayerjeva aplikacijska koda se prepiše v Rust brez namernega
izpuščanja funkcij. Windows izdaja mora najprej doseči popolno pariteto s Python
baselineom. Razvojne alphe in bete se gradijo ter nameščajo samo lokalno in se ne
objavljajo na GitHubu. Spotify trenutno ni v obsegu.

Izvedbeni plan: `docs/RUST_REWRITE_PLAN.md`.

Obvezni acceptance manifest: `docs/RUST_PARITY_MANIFEST.md`.

## macOS feature-parity port

Status: potrjeno prihodnje delo po Windows Rust parity gateu; implementacija se
ni začela.

ApricotPlayer mora dobiti macOS arm64 izdajo z vsemi funkcijami trenutne Windows
izdaje. Port mora ohraniti en skupni codebase, VoiceOver in popolno tipkovnično
dostopnost, Command namesto Control, native macOS sistemske integracije, DMG ter
skupen Windows/macOS release pipeline. Prvi DMG je lahko ad-hoc podpisan in
nenotariziran, dokler projekt nima Apple Developer računa; release mora jasno
opisati Gatekeeper opozorilo. Noben obstoječ feature ne sme biti namenoma
izpuščen.

Celoten macOS acceptance manifest je v `docs/MACOS_PORT_PLAN.md`. Njegovo staro
Python/wxPython izvedbeno zaporedje nadomesti Rust-first zaporedje iz
`docs/RUST_REWRITE_PLAN.md`; funkcijska pariteta ostane nespremenljiva.

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
