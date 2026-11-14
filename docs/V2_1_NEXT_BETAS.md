# Funkcije za bete po 2.1.0-beta.2

Urh se je 14. 11. 2026 odločil, da gredo v beto 2 samo izbira velikosti skoka (custom seek), skoki na odstotek, globalne bližnjice ter hiter vklop odstranjevanja tišine in prostorskega zvoka. Spodnje funkcije pridejo pozneje, po sklopih, v naslednjih betah ali po 2.1, odvisno od stabilnosti. Vse so lastna implementacija; bližnjice je treba izbrati brez konfliktov na Windows in Macu (tipka F je na primer že prikaz formata).

## Predvajanje in navigacija

- Bližnjici za največjo in najmanjšo glasnost.
- Možnost, da se ob Previous in Next prebere ime dosežene datoteke.
- Večkratni pritisk ene tipke: najprej ime, nato pot oziroma naslov toka, nato kopiranje poti.
- Okno z vsemi odprtimi datotekami (skok na izbrano, podatki o seznamu: število, velikost, skupni, pretečeni in preostali čas).

## Datoteke

- Odpiranje ene datoteke iz Raziskovalca ali Finderja z izbiro: samo datoteka, datoteke iste mape ali mapa s podmapami, izbrana datoteka ostane izbrana. Razlikuje se od obstoječega Play file, ki vedno odpre samo eno datoteko brez sosednjih, zato Next ne gre na naslednjo datoteko v mapi.
- Nedavno odprte datoteke, mape in playlisti, vsak seznam s svojim brisanjem.
- Odpiranje datoteke ali mape iz odložišča in kopiranje trenutne datoteke v odložišče (kot datoteka za Raziskovalec ali Finder).
- Preimenovanje trenutne datoteke, prikaz v Raziskovalcu ali Finderju, lastnosti datoteke (na Macu Get Info).
- Označevanje datotek med poslušanjem in dejanja nad označenimi (kopiraj, premakni, v odložišče, izbriši).

## YouTube

- Odstranjevanje sponzorskih odsekov (SponsorBlock) pri prenosu in vgradnja poglavij.
- Odpiranje videa in kanala v brskalniku ter okno z opisom videa.
- Kanal posodobitev za yt-dlp: stable, nightly ali master.
- Predlogi med tipkanjem v polju za iskanje (zdaj na Ctrl+Shift+Down), z možnostjo izklopa.
- Stalna izbira za povezavo z videom in seznamom: vprašaj, vedno video ali vedno seznam.
- Med priljubljenimi poljuben spletni tok ter urejanje imena in povezave.

## Ostalo

- Razlaga nastavitve na F1, ko je fokus na njej.
- Dve ravni zgovornosti (polna in kratka) s hitrim preklopom.
- Pri IPTV možnost HLS za kanale Xtream (iskanje kanalov že obstaja).
- Pri equalizerju izbira izhodiščnega profila ob novem profilu ter dodatni profili za slušalke, govor in tiho poslušanje.

## Že obstaja, zato ni v seznamu

- Korak ravnovesja in prostorskega zvoka: okno Audio processing (F5).
- Brisanje izvirnikov po pretvorbi: "Replace the original files" v pretvorniku.
- Iskanje kanalov IPTV: ukaz Search v seznamu IPTV.
