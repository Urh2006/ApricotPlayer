# P7 dokazi: zasebnost, zmogljivost, paket, manifest

2. 10. 2026, verzija 2.0.0-beta.11.

## Zasebnost

- `spotify/spotify.log` (469 vrstic po vseh preizkusih): 0 zadetkov za
  `bearer`, `access_token`, `auth_data`, `client-token`, `refresh_token` in
  dolge žetone; ni uporabniškega imena. Dnevnik ima omejeno velikost.
- `spotify_accounts.json`: ključ računa, prikazno ime, Premium, država in
  poverilnice samo kot DPAPI blob (792 znakov); `device_id` brez osebnih
  podatkov.
- Apricotove priljubljene in zgodovina hranijo samo trajne Spotify povezave in
  imena, nikoli žetonov ali CDN povezav.
- LibreSpot zvočni predpomnilnik je izklopljen, ker bi poverilnice zapisal v
  čistem besedilu.
- Diagnostično poročilo: razdelek Spotify ima samo število računov, stanje
  prijave in povezave, Premium in nastavitve.

## Zmogljivost (izdajna gradnja, nevidno namizje, glasnost 0)

```
window shown after 216 to 224 ms (z računom Spotify ali brez; prvi hladni zagon 2475 ms)
idle after start: working set 39 MB
Liked Songs listed after 904 ms
Playing after 2195 ms (Enter na skladbi do "Playing:")
5 min predvajanja Všečkanih skladb (trije prehodi skladb): private memory 83 do 85 MB, brez rasti
average CPU over playback: 0,42 % vseh jeder
exited 212 ms after WM_CLOSE
```

## Paket

Namestitveni program (isti AppId kot Python ApricotPlayer) se je tiho namestil
čez prejšnjo verzijo (beta.9, beta.10, beta.11, izhodna koda 0). LibreSpot je
statično povezan, paket nima novih DLL-jev.

## Manifest

`docs/SPOTIFY_PARITY_MANIFEST.md`: vse vrstice S01 do S55 imajo status.
Odprto: S31 profili, S42 sprotna osvežitev ob spremembi s telefona, S44 zvočne
knjige, S46 prepisi, S48 predogledi, S35 opis in vidnost seznama, S41
razvrščanje, mešana Apricot zaporedja s Spotify samodejnim predvajanjem (S51)
ter Urhovi preizkusi z NVDA, brajlom in telefonom (`docs/SPOTIFY_TEST_GUIDE.md`).
S49 lokalne datoteke Spotify ne podpira.
