# Spotify audit za 2.0.0-beta.2

Audit zajema Spotify core/app modele, accounts/OAuth, catalog in library edit API, sejo in Connect, PCM predvajanje z libmpv, queue/Smart Shuffle, naprave, nastavitve, diagnostiko ter Windows zaslone in bližnjice. Izhodišče je implementacija od `a6562ff` do beta 1 (`2898684`); popravki so pripravljeni na veji `beta`. Python ostaja referenca za skupno navigacijo in predvajanje, Spotify pa je nova integracija.

## Potrjene napake, popravljene v beta 2

1. **P1: blokirana dekoderska nit po stop/zamenjavi.** Bounded PCM ring je čakal na mpv reader, ki ga ni bilo več. `stop` in nov lasten začetek zapreta opuščeno generacijo. Ponovno nadaljevanje ne uporabi zaprte generacije. Offline test s polnim ringom je pred popravkom odpovedal, po njem uspe.
2. **P1: razhajanje Spotify/mpv pavze.** mpv se je ustavil pred potrditvijo Spotify pavze, kar je lahko blokiralo decoder pred obdelavo naslednjih ukazov. PCM pause zdaj počaka na potrditev vira. Vsak potrjeni Spotify Playing ponovno uskladi mpv pavzo, tudi pri Next/Previous.
3. **P1: zastarel EOF zaključi novo skladbo.** `eof-reached` je zaupal zapoznelemu payloadu prejšnje skladbe. Zdaj preveri tudi dejansko trenutno libmpv lastnost. Test z dejanskim libmpv je pred varovalom vrnil `Ended` za zastarel EOF, po njem dogodek zavrne.
4. **P1: mrtva Connect seja ostane nameščena.** Spontani konec Spirc je samo oglasil disconnect, `connected_account()` pa je ostal Some. Naslednji play je zato preskočil reconnect in pošiljal ukaze mrtvi seji. Konec zdaj odstrani samo svojo sejo in PCM vir; konec stare zamenjane seje ne odstrani nove.
5. **P1: zastarel connect/login prepiše novejši račun.** UI stamp je preverjal rezultat šele po zapisovanju računa in menjavi seje. Service ima svojo lifecycle generacijo in serializira preverjanje, zapis ter namestitev. Cancel/logout/remove/shutdown razveljavijo stare poskuse in zaprejo odvečne odprte seje. Test preveri tudi, da zastarel poskus ne obnovi odstranjenega računa.
6. **P1/P2: predpomnilniki in odgovori prečkajo menjavo računa.** Editable playlist picker, hidden songs, search in frame stack so ostali od prejšnjega računa. Menjava jih počisti; catalog/edit/resolve/queue/transfer/smart odgovori morajo pripadati trenutnemu account epochu.
7. **P2: brisanje spremeni pagination offset.** Po unlike ali odstranitvi occurrence je strežnik premaknil naslednje skladbe za eno mesto, klient pa je nadaljeval s starim offsetom. Cursor se zmanjša za dejansko odstranjene vrstice; prejšnji pending page/refresh odgovor se zavrže. Test simulira server deletion in preveri, da track 50 ostane prva neprebrana skladba.
8. **P2: Previous preskoči Smart Shuffle priporočilo.** Vendored Connect history je ohranil le context/autoplay provider. Zdaj ohrani tudi smart priporočila naprej in nazaj; regression preveri original → priporočilo → original → Previous → priporočilo.
9. **P2: Play na Smart Shuffle vrstici queue išče neobstoječ context UID.** Priporočilo ne pripada izvornemu playlistu. Zdaj se njegova točna queue occurrence prestavi na začetek in predvaja z Next; ročna vrsta in ostali context ostanejo. Normalnih context/autoplay vrstic ta premik ne dovoljuje.
10. **P2: napačna identiteta library folderja.** Filtriranje neimenovanih vrstic pred zip z originalnim JSON je premaknilo identitete. Folder se popravi proti svojemu raw elementu pred filtriranjem. Sintetični test je pred popravkom odpovedal.
11. **P2: albumi tiho končajo po 300 skladbah.** Catalog request in UI zdaj ohranita offset in next_offset tudi za albume. Synthetic collection test pokriva album z več kot 300 skladbami.
12. **P2: Direct link zavrne podprte kolekcije.** Artist/show/profile parser in browse že obstajata, play_link pa ju je zavrnil. Po povezavi se odpre pripadajoči obstoječi seznam.
13. **Background Escape in shortcuts.** Escape na action kontroli je vedno zaprl runtime ne glede na nastavitev. Zdaj ob vklopljenem background playbacku zapusti player brez stop. Ctrl+PageUp/Down sta na voljo tudi na rezultatih; route/global actions imajo prednost in navadne črke/native navigacija ne postanejo background player shortcuti. Close kontrola ostaja izrecni stop in nima več zavajajoče oznake Escape.

## Kaj še ostaja

- **P2: globalne Spotify nastavitve.** `settings.rs` še uporablja en `spotify/settings.json`, kljub potrjeni zahtevi D10 za account-local nastavitve. Quality, normalization, autoplay in library order se delijo med računi. Migracijo je treba pripraviti tako, da ohrani obstoječe vrednosti.
- **P2: ponovni začetek končanega Smart Shuffle priporočila.** Trenutni item ohrani originalni context in smart UID, ki ga originalni playlist nima. Aktivni seek na začetek je pravilna pot; novo nalaganje že končanega priporočila lahko izbere prvo originalno skladbo. Potreben je ločen popravek strategije resume z ohranitvijo contexta.
- **P2: create playlist in že odprta paginirana knjižnica.** Lokalno vstavljanje novega playlista na začetek ne upošteva vseh library filtrov, vrstnega reda in pending offsetov. Potreben je reload/invalidation kolekcije z ohranitvijo izbire, namesto ugibanja strežniške pozicije.
- **Nezaključena funkcionalnost:** artist discography uporablja omejen overview; audiobooks/chapters so že označeni kot nepodprti v parity manifestu. To ni dokaz popolne Spotify feature parity.
- **Diagnostika:** Warn/Error filter sam ne zagotavlja, da LibreSpot nikoli ne izpiše Spotify URI ali osebnih metapodatkov. Raw logger nima vsebinske redakcije. Med auditom nisem pregledoval osebnih logov ali poverilnic; ne trdim, da je prišlo do izpostavitve skrivnosti.

## Meje potrditve

Uporabnikova konkretna živa Spotify napaka first → Next → Previous ni bila odigrana s prijavljenim računom. Navadni Connect model za to zaporedje uspe v offline testu; integracijska EOF in pause odstopanja so potrjena z ločenimi regresijami. Najdene blokade in mrtva seja lahko pojasnijo predvajanje, ki deluje šele po restartu, vendar brez loga dogodka vzroka njegovega primera ni mogoče dokončno pripisati.

Like uporablja Spotify save/unsave pot; Hide song in Unlike sta ločeni dejanji. Live potrditev sinhronizacije Liked Songs z uradnim Spotify klientom, recovery po dejanski izgubi omrežja, ter NVDA govor/fokus pri Next/Previous ostajajo ročni acceptance koraki.

## Preverjanje

- Python compileall, 123 regresijskih testov in kritični Ruff: uspešno.
- Rust workspace: 710 uspešnih testov, 27 privzeto izključenih testov za zunanje runtimes/omrežne račune; brez odpovedi.
- Vendored LibreSpot Connect: 4 unit testi in 1 doctest uspejo.
- Dejanski libmpv: vseh 10 integracijskih testov uspe, vključno z stale EOF/deferred pause, PCM seek, koncem generacije in naravnim koncem.
- Clippy za workspace/all-targets z -D warnings, cargo fmt --check in git diff --check: uspešno.
- Pravi UI baseline beta 1: Escape na kontroli Previous ob vklopljenem background playbacku odstrani background player; napaka je reproducirana.
- Release uporablja build_stable_release.ps1 -Channel beta iz čistega commita; installer in ZIP nastaneta iz istega svežega paketa. Živi Spotify/NVDA acceptance ostaja ločen od avtomatskih testov.
- Prvi celotni Rust build je odpovedal zaradi skoraj polnega diska; ustvarjeni debug artefakti so bili počisteni in preverjanje se izvaja ponovno brez debug simbolov. To ni bila odpoved funkcijskega testa.
