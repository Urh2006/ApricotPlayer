# Trajne projektne odločitve

## D-001: Dostopnost je osnovna zahteva

Vse uporabniške funkcije morajo delovati s tipkovnico in NVDA. Focus order, dostopna imena, kontekstni meniji, Escape vedenje in screen-reader obvestila so del funkcionalnosti, ne naknadna izboljšava.

## D-002: Beta razvoj ostane ločen od stable veje

Predizdaje se pripravljajo na `beta` in objavljajo kot prerelease, stabilne izdaje pa na `main`. Updater na stable kanalu ne sme ponujati beta izdaje.

## D-003: Release ima dve nespremenljivi imeni artefaktov

Vsak release vsebuje:

- `ApricotPlayerSetup.exe`
- `ApricotPlayer.zip`

Updater in uporabniška dokumentacija se zanašata na ti imeni.

## D-004: Uporabniški podatki imajo prednost pred udobjem implementacije

Nastavitve, zgodovina, favorites, playlisti, queue, bookmarks, subscriptions, RSS podatki, pozicije in zadnja seja se zapisujejo varno in se pri namestitvi ali posodobitvi ne smejo izgubiti. Updater mora preveriti paket in omogočiti rollback celotnega runtimea.

## D-005: Običajna predvajalna pot mora ostati hitra

Fallbacki za cookies, alternativne YouTube cliente, HLS ali poškodovane streame se sprožijo šele po dokazani napaki. Običajen uspešen video ne sme zaradi njih dobiti dodatnega extraction klica ali zamika.

## D-006: Trenutni repo ima prednost pred starimi chati

Stari chati so pomembni za namen in razloge, vendar vsebujejo tudi že odpravljene buge, stare poti, prejšnjo monolitno arhitekturo in zaključene načrte. Ob konfliktu veljajo trenutna koda, testi, Git zgodovina, changelog in release notes.

## D-007: AudioVault spada v 1.0

AudioVault je potrjen pomemben feature ApricotPlayerja 1.0. Integracija je bila razvita in stabilizirana skozi beta izdaje ter ostaja del stabilnega izdelka.

## D-008: macOS mora imeti popolno funkcijsko pariteto

macOS izdaja ni okrnjena različica ApricotPlayerja. Imeti mora vse funkcije,
nastavitve, shortcute, kontekstne menije, podatkovne operacije in dostopne
tipkovnične poti, ki obstajajo na Windows. Windows uporablja Control, macOS pa
Command kot primarni modifier. Razlike so dovoljene samo za native sistemske
površine, kjer mora macOS implementacija ohraniti isto uporabniško dejanje.

Port ostane v enem skupnem codebaseu s platformnimi adapterji. Po prvi macOS
izdaji mora biti vsak nov feature implementiran in preverjen na obeh platformah
v istem razvojnem ciklu. Celoten obseg in acceptance manifest sta v
`docs/MACOS_PORT_PLAN.md`.

## D-009: macOS javni paket je DMG

Windows release ohrani nespremenljivi imeni `ApricotPlayerSetup.exe` in
`ApricotPlayer.zip`. Ko je macOS release pipeline aktiviran, isti release doda
`ApricotPlayer-macOS-arm64.dmg`. Ločen macOS ZIP ni del zahtevanega javnega
releasea.

Dokler ni Apple Developer računa, je lahko DMG ad-hoc podpisan in nenotariziran.
Release mora to jasno povedati in opisati pričakovano Gatekeeper opozorilo ter
uradno pot za enkratno odobritev v macOS System Settings. Lastna kriptografska
preverjanja updaterja ostanejo obvezna. Developer ID podpis in notarizacija sta
poznejša izboljšava ter ne blokirata prve zasebne ali zgodnje javne macOS izdaje.
