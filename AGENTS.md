# Codex navodila za ApricotPlayer

Ta datoteka je trajni repozitorijski kontekst za Codex. Velja za celoten projekt.

## Komunikacija

- Vsak uporabniku viden odgovor začni z naslovom prve ravni `# Codex je odgovoril`, ker uporabnik navigira z bralnikom zaslona.
- Privzeto odgovarjaj v slovenščini. Drug jezik uporabi, če ga uporabnik zahteva ali če pripravljaš vsebino za tuje uporabnike.
- Najprej povej rezultat oziroma trenutno ugotovitev, nato podrobnosti.

## Produkt in prioritete

ApricotPlayer je dostopen medijski predvajalnik in downloader za Windows. Narejen je v Pythonu z wxPython, za predvajanje uporablja mpv, za spletne medije pa predvsem yt-dlp.

Prioritete so po vrstnem redu:

1. Dostopnost za NVDA in druge bralnike zaslona.
2. Pravilna tipkovnična navigacija in predvidljivo ohranjanje fokusa.
3. Zanesljivo predvajanje, iskanje, prenosi, uporabniški podatki in posodobitve.
4. Hiter začetek predvajanja in brez dodatnega dela na običajni uspešni poti.
5. Jasne, lokalizirane in uporabne napake.

Vsaka nova ali spremenjena funkcija mora biti izvedljiva brez miške. Kjer obstaja kontekstni meni, mora delovati tudi z Applications key oziroma `Shift+F10`. Kontrole morajo imeti smiselna dostopna imena, spremembe se ne smejo po nepotrebnem oglašati večkrat, fokus pa se ne sme premikati zaradi osveževanja podatkov v ozadju.

## Struktura

- `wx_main.py` je vstopna točka in sestavi `MainFrame` iz mixinov.
- `apricot/ui/` vsebuje zaslone, dialoge, menije in UI mixine.
- `apricot/player/` vsebuje mpv, playback, EQ, glasnost in sorodno logiko.
- `apricot/network/` vsebuje YouTube, cookies in spletne integracije.
- `apricot/download/`, `library/`, `media/`, `search/`, `system/` in `updater/` vsebujejo pripadajoče domenske sklope.
- `locales_json/` je vir lokaliziranih besedil.
- `tests/` vsebuje regresijske teste.
- `docs/PROJECT_CONTEXT.md`, `docs/DECISIONS.md` in `docs/BACKLOG.md` vsebujejo zgoščen kontekst, trajne odločitve in potrjeno prihodnje delo.

Za trenutno verzijo vedno preberi `apricot/__init__.py`, `apricot/constants.py`, vrh `CHANGELOG.md` in zadnje release notes. Stari chati niso vir resnice, kadar se razlikujejo od trenutne kode, Git zgodovine ali teh dokumentov.

## Pravila dela

- Pred spremembami preveri `git status` in ohrani vse uporabnikove nepovezane ali nesledene spremembe.
- Posebej ne spreminjaj ali vključuj `release-notes/v0.9.44.md`, dokler uporabnik tega izrecno ne zahteva.
- Ne urejaj lokalnih buildov, ekstrahiranih EXE vsebin, recovery kopij ali starih diagnostičnih skript kot nadomestilo za spremembo izvorne kode.
- Najprej sledi dejanskemu toku podatkov in stanju predvajalnika. Ne ugibaj na podlagi starega monolitnega `wx_main.py`, če je bila logika že preseljena v mixin.
- Izogibaj se širokim refaktorjem med ozkim bugfixom. Ohrani vedenje, ki ni povezano z zahtevo.
- Ne dodajaj čakanja, dodatnega yt-dlp klica, cookies poti ali omrežnega fallbacka na običajno uspešno pot brez dokazane potrebe.
- Uporabniške nastavitve, zgodovina, favorites, playlisti, pozicije, bookmarks, subscriptions in RSS podatki se pri posodobitvah ne smejo izgubiti ali prepisati.
- Vsako novo običajno možnost glavnega menija dodaj v `MAIN_MENU_CUSTOMIZABLE_ITEMS`, da jo lahko uporabnik skrije v sekciji Customize main menu. Skrivanje ne sme onemogočiti pripadajoče globalne bližnjice. Razpoložljiva posodobitev, `Settings` in `Exit` morajo vedno ostati vidni in zato ne sodijo v nastavljivi katalog.

## Preverjanje

Preverjanje prilagodi tveganju spremembe. Osnovni nabor je:

```powershell
& .\.venv\Scripts\python.exe -m compileall -q wx_main.py apricot
& .\.venv\Scripts\python.exe -m unittest discover -s tests -v
& .\.venv\Scripts\python.exe -m ruff check apricot wx_main.py tests scripts/zip_folder.py --select E9,F63,F7,F82,F811,E402
git diff --check
```

Če projektni `.venv` ni na voljo, uporabi drug interpreter šele po preverjanju, da ima nameščene `requirements-dev.txt`. Celoten nefiltriran Ruff trenutno ni baseline projekta; obvezna avtomatska vrata so kritične kategorije iz security workflowa, nove ali spremenjene datoteke pa naj bodo čiste tudi širše, kadar je to izvedljivo brez nepovezanega refaktorja.

Pri UI, fokusu ali tipkovnični navigaciji dodaj pravi wxPython smoke test. Pri playerju preveri ustrezno mpv pot. Pri updaterju preveri uspešno zamenjavo in prisiljen rollback. Pri release spremembah preveri installer in portable ZIP iz istega svežega builda.

## Verzije in objave

- Do stabilne izdaje 1.0 se razvojne izdaje pripravljajo na veji `beta`; `main` ostane stabilen.
- Navadna analiza, dokumentacija ali lokalni popravek se ne objavi samodejno. Commit, push, tag, GitHub release ali lokalno namestitev naredi samo, ko uporabnik zahteva objavo oziroma novo verzijo.
- Pri novi verziji uskladi `apricot/__init__.py`, verzijske konstante, README, `CHANGELOG.md` in `release-notes/v{verzija}.md`.
- Beta izdaja mora biti GitHub prerelease in mora biti zgrajena iz pravega beta commita oziroma taga.
- Vsak release mora vsebovati točno `ApricotPlayerSetup.exe` in `ApricotPlayer.zip`.
- Issues zapri šele, ko je popravek objavljen in so build, testi ter oba release artefakta preverjeni.

## Obseg različice 1.0

Obstoječi `docs/ROADMAP_1.0.md` je izveden, vendar stabilna 1.0 še ni zaključena. AudioVault je potrjen pomemben del 1.0 in mora biti najprej razvit ter stabiliziran skozi beta izdaje. Podrobnosti so v `docs/BACKLOG.md`.
