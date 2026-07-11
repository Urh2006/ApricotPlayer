# Trajne projektne odločitve

## D-001: Dostopnost je osnovna zahteva

Vse uporabniške funkcije morajo delovati s tipkovnico in NVDA. Focus order, dostopna imena, kontekstni meniji, Escape vedenje in screen-reader obvestila so del funkcionalnosti, ne naknadna izboljšava.

## D-002: Beta razvoj ostane ločen od stable veje

Do stabilne 1.0 se nove razvojne izdaje pripravljajo na `beta` in objavljajo kot prerelease. `main` mora ostati stabilen. Updater na stable kanalu ne sme ponujati beta izdaje.

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

AudioVault je potrjen pomemben feature za ApricotPlayer 1.0. Integracija bo najprej nastajala in se preverjala v beta izdajah; stabilna 1.0 ne pomeni samo zaključka starega roadmapa, ampak tudi primerno stabiliziran AudioVault.
