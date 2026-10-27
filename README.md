# Sorobo – Buchhaltung

Lokale Buchhaltungssoftware für Selbstständige, Freiberufler und kleine Unternehmen. Läuft
vollständig auf dem eigenen Rechner (keine Cloud), die Daten liegen in einer SQLite-Datenbank.

Projektwebsite: [sorobo.de](https://sorobo.de).

## Funktionen

- **Verkauf:** Angebote, Auftragsbestätigungen, Lieferscheine, Rechnungen, Abschlags- und
  Schlussrechnungen, Gutschriften und Stornorechnungen – inklusive Umwandlung entlang des
  Vorgangs (Angebot → Auftrag → Lieferschein/Rechnung). Abo-Rechnungen mit automatischem
  Abschluss und E-Mail-Versand, Preisanpassungen und Platzhaltern für den Leistungszeitraum.
- **E-Rechnungen:** Erzeugen von ZUGFeRD (PDF/A-3 mit eingebettetem XML) und XRechnung 3.0
  (CII, EN 16931), Einlesen von ZUGFeRD/XRechnung bei der Belegerfassung.
- **Layouts:** Layout-Designer mit frei positionierbaren Elementen, Logo, Briefpapier (PDF/Bild),
  Schriften und Farben. PDF-Satz mit [Typst](https://typst.app).
- **Offene Posten & Mahnwesen:** Altersstruktur, Zahlungserinnerungen und Mahnstufen mit
  Gebühren und Verzugszinsen, Mahnlauf, GiroCode auf Rechnungen und Mahnungen.
- **Einkauf:** Belegerfassung (Upload, Kategorisierung, Aufteilung nach Steuersätzen, § 13b),
  Lieferanten automatisch aus E-Rechnungen anlegen.
- **Bank & Kasse:** CSV- und CAMT-Import mit automatischer Zuordnung, Import der Berichte von
  PayPal, Stripe und SumUp (Gebühren und Auszahlungen werden gebucht), Zahlungen, Skonto,
  Ausbuchungen, SEPA-Überweisungsdateien (pain.001) für offene Belege, SEPA-Lastschriften
  (pain.008) mit Mandatsverwaltung, Vorabankündigung und Erkennung von Rücklastschriften,
  GoBD-konformes Kassenbuch mit Stornobuchungen.
- **Buchhaltung:** Einnahmen-Überschuss-Rechnung (Ist-Versteuerung) oder doppelte Buchführung
  (Bilanzierung, Soll-Versteuerung), SKR03/SKR04, Buchungsjournal, Kontenblätter, manuelle
  Buchungen, Anlagenverzeichnis mit AfA (linear, GWG, Sammelposten), Festschreibung.
- **Steuern & Berichte:** Umsatzsteuer-Voranmeldung (inkl. Dauerfristverlängerung und
  Kleinunternehmerregelung), Zusammenfassende Meldung (CSV-Import für ELSTER), EÜR, GuV, Bilanz, BWA, Summen- und Saldenliste, Auswertungen,
  Dashboard, DATEV-Export (EXTF-Buchungsstapel), GoBD-Datenexport für Betriebsprüfungen
  (Beschreibungsstandard, index.xml), Beleg-ZIP, alle Berichte als PDF.

## Schnellstart (Windows)

Fertige Windows-Installer und portable Archive stehen nach einem Release unter **Releases**
im GitHub-Repository bereit. Der Installer installiert Sorobo für den aktuellen Benutzer
(ohne Administratorrechte) und legt einen Eintrag im Startmenü sowie optional auf dem Desktop an.

Zum eigenen Bauen: [Rust](https://rustup.rs) (stable, unter Windows x64 MSVC),
[Node.js](https://nodejs.org) ab Version 22 und [Inno Setup 6](https://jrsoftware.org/isdl.php).
`ISCC.exe` wird im PATH oder im Standardordner von Inno Setup gesucht; alternativ die
Umgebungsvariable `ISCC` auf den vollständigen Pfad setzen.

```bat
build.cmd
```

baut Frontend und Backend und legt die fertigen Programme unter `dist\` ab:

- **`Sorobo.exe`** – die Desktop-Anwendung. Ein Doppelklick (oder `start.cmd`) öffnet Sorobo in
  einem eigenen Fenster – ohne Konsole und ohne Browser. Ein erneuter Start holt das bereits
  geöffnete Fenster in den Vordergrund. PDFs zum Drucken öffnen sich in einem eigenen Fenster,
  Downloads fragen mit dem Dialog „Speichern unter“ nach dem Ziel. Beim Schließen mit
  ungespeicherten Änderungen fragt Sorobo nach.
- **`sorobo-server.exe`** – derselbe Server ohne Fenster, z. B. für die Nutzung im Browser
  (<http://localhost:8787>) oder für die Entwicklung.
- **`packages\Sorobo-<Version>-windows-x64-setup.exe`** – Windows-Installer mit beiden
  Programmen und Lizenztexten. `build.cmd --no-installer` baut nur die portablen Programme.
  Nach einem bereits fertigen Build kann mit
  `powershell -NoProfile -ExecutionPolicy Bypass -File scripts\build-installer.ps1`
  nur der Installer neu erstellt werden.

Die Daten liegen im Ordner `dist\data` neben den Programmen (beide nutzen dieselben Daten). Zum
Umziehen oder Sichern genügt es, diesen Ordner zu kopieren – oder unter
*Einstellungen → Daten & Sicherung* eine Sicherung herunterzuladen.

Bei Installation ist der Standardpfad `%LOCALAPPDATA%\Programs\Sorobo`, die Daten liegen
entsprechend in `%LOCALAPPDATA%\Programs\Sorobo\data`. Updates über denselben Installer
ersetzen nur die Programmdateien. Auch eine Deinstallation lässt die Daten bestehen.
Beim Wechsel von einer portablen Version Sorobo schließen und den bisherigen `data`-Ordner
in den Installationsordner kopieren, bevor die installierte Anwendung gestartet wird.

Sorobo fragt beim Start und danach täglich bei GitHub nach einer neuen Version und meldet
sich, wenn es eine gibt (abschaltbar unter *Einstellungen → Updates*; Buchhaltungsdaten werden
dabei nicht übertragen). Dort lässt sich das Update auch direkt installieren: Sorobo legt vorher
eine Sicherung an, lädt das Paket für das eigene System, prüft dessen SHA-256-Prüfsumme und
startet danach neu – eine Installation über den neuen Installer, portable Programme und
`.tar.gz`-Archive durch Austausch der Programmdateien.

Die Desktop-Anwendung benötigt die „Microsoft Edge WebView2 Runtime“, die auf vielen
Windows-Systemen bereits vorhanden ist. Falls sie fehlt, die
[Evergreen Runtime von Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/)
installieren; sie ist nicht im Sorobo-Installer enthalten. Unter Linux/macOS entsprechend
`sh build.sh`; Linux benötigt dafür `webkit2gtk-4.1` und GTK 3 (zum Bauen die jeweiligen
Entwicklungspakete). Mit `SOROBO_SERVER_ONLY=1 sh build.sh` wird nur der Server gebaut.
Das Skript erzeugt zusätzlich ein portables `.tar.gz` unter `dist/packages/`. Linux-Archive
benötigen auch auf dem Zielsystem WebKitGTK 4.1/GTK 3; macOS-Archive enthalten native Programme,
kein `.app`-Bundle. Die Pakete sind derzeit weder signiert noch unter macOS notarisiert.

Beim ersten Start führt ein Assistent durch die Einrichtung. Mit *„Erst einmal ausprobieren“*
wird ein vollständiges Demo-Geschäftsjahr geladen.

### Umstieg von Sqonto

Bis Version 0.10 hieß das Programm *Sqonto*. Bestehende Installationen lassen sich wie gewohnt
über *Einstellungen → Updates* oder den neuen Installer aktualisieren:

- Die Datenbank `sqonto.db` wird beim ersten Start unverändert als `sorobo.db` übernommen.
  Sicherungen aus Sqonto lassen sich weiterhin wiederherstellen, alte automatische Sicherungen
  werden mit den neuen rotiert.
- Der Installer ersetzt `Sqonto.exe` und `sqonto-server.exe` durch die neuen Programme und
  verschiebt die Verknüpfungen; der Installationsordner (`Programs\Sqonto`) mit den Daten bleibt.
- Portable Installationen behalten bei Updates ihre bisherigen Dateinamen, damit Verknüpfungen
  und Skripte weiter funktionieren. Wer möchte, benennt die Programme nach dem Beenden um.
- Die Umgebungsvariablen `SQONTO_*` werden weiterhin gelesen, wenn `SOROBO_*` nicht gesetzt ist.

## Konfiguration

| Umgebungsvariable    | Bedeutung                                                                | Standard                       |
| -------------------- | ------------------------------------------------------------------------ | ------------------------------ |
| `SOROBO_DATA_DIR`    | Datenverzeichnis (Datenbank, Dateien, Protokoll `sorobo.log`)            | `data` neben der Programmdatei |
| `SOROBO_PORT`        | Port des lokalen Servers (ist er belegt, wird der nächste freie genutzt) | `8787`                         |
| `SOROBO_DESKTOP_URL` | Desktop-Fenster zeigt diese Adresse statt des eingebauten Frontends      | –                              |
| `SOROBO_DEVTOOLS`    | gesetzt: Entwicklerwerkzeuge im Desktop-Fenster                          | nur in Debug-Builds            |

`sorobo-server --open` öffnet nach dem Start zusätzlich den Browser.

Der Server lauscht ausschließlich auf `127.0.0.1` und ist damit nur vom eigenen Rechner aus
erreichbar – auch wenn er in der Desktop-Anwendung läuft.

## Entwicklung

Backend und Frontend lassen sich weiterhin getrennt starten:

```bash
# Backend (Port 8787), Daten im Arbeitsverzeichnis unter ./data
cd backend && cargo run

# Frontend mit Hot Reload (Port 5173, leitet /api an das Backend weiter)
cd frontend && npm install && npm run dev
```

Die Oberfläche dann im Browser unter <http://localhost:5173> öffnen – oder im Desktop-Fenster:

```bash
cd backend && SOROBO_DESKTOP_URL=http://localhost:5173 cargo run --bin sorobo-desktop
```

Die TypeScript-Typen der API werden aus den Rust-Strukturen erzeugt (`ts-rs`):

```bash
cd frontend && npm run types
```

Tests: `cd backend && cargo test`, Typprüfung: `cd frontend && npm run typecheck`.

### GitHub Actions und Releases

`.github/workflows/build.yml` läuft bei Pushes, Pull Requests und manuell. Auf Windows x64,
Linux x64 (Ubuntu 24.04) und macOS ARM64 werden Frontend-Typprüfung und Frontend-Build,
Rust-Tests aller Targets und der Release-Build ausgeführt. Tests laufen mit `--release`,
damit sie ihre kompilierten Abhängigkeiten mit dem anschließenden Paket-Build teilen.
Ein Rust-Cache pro Plattform hält Abhängigkeiten und kompilierte Artefakte zwischen Läufen
vor; Änderungen am Compiler oder den Abhängigkeiten werden bei der Cache-Auswahl berücksichtigt.
Die Build-Skripte verwenden in CI mit `SOROBO_FRONTEND_READY=1` das bereits gebaute Frontend
(lokale Builds bauen es weiterhin selbst). Unter Linux werden nach dem Paket-Build zusätzlich
die Bibliothek und der Server ohne Desktop-Features im Release-Profil getestet.
Unter Windows prüft ein zusätzlicher Test Installation, Start des installierten
Servers mit eingebettetem Frontend, Update (auch mit Neustart wie beim Selbst-Update) und
Datenerhalt bei Deinstallation. Die fertigen
Pakete können bei jedem erfolgreichen Lauf als Workflow-Artefakte heruntergeladen werden.

Für einen Release:

1. Dieselbe Version `X.Y.Z` in `backend/Cargo.toml` und `frontend/package.json` setzen,
   die Lockdateien aktualisieren und die Änderungen committen.
2. Den Tag `vX.Y.Z` erstellen und zu GitHub pushen, z. B. `git tag v0.11.0` und
   `git push origin v0.11.0`.
3. Nach erfolgreichen Builds aller Plattformen veröffentlicht die Pipeline automatisch
   einen GitHub Release mit Windows-Installer, Windows-ZIP, Linux/macOS-Archiven,
   SHA-256-Prüfsummen und automatisch erzeugten Release Notes. Der Tag muss exakt zur
   Paketversion passen. Zum Wiederholen eines fehlgeschlagenen Releases den Workflow des
   Tags erneut ausführen; ein manueller Branch-Lauf veröffentlicht keinen Release.

Der Workflow benötigt keine eigenen Secrets: nur der Release-Job erhält Schreibrechte
über den eingebauten `GITHUB_TOKEN`. GitHub Actions muss im Repository aktiviert sein.
Die Paketlisten enthalten ausschließlich Programme und Lizenztexte; lokale `data`-Ordner
werden nicht verpackt.

### Aufbau

- `backend/` – Rust-Bibliothek `sorobo` (Axum, SQLite über rusqlite, rust_decimal) mit zwei
  Programmen: `src/main.rs` (`sorobo-server`) und `src/bin/sorobo-desktop.rs` (Fenster mit
  WebView2/WebKit über `wry`/`tao`). Buchungslogik in `src/services/posting.rs`, Berichte in
  `src/services/reports.rs`, E-Rechnung in `src/services/einvoice.rs`, PDF-Erzeugung mit
  eingebettetem Typst in `src/pdf/` und `templates/`.
- `frontend/` – React, TypeScript, Mantine, TanStack Query. Das fertige Frontend wird beim
  Release-Build in die Programme eingebettet; `src/lib/desktop.tsx` verbindet es mit dem
  Desktop-Fenster (ungespeicherte Änderungen beim Schließen, Download-Hinweise).

## Lizenz

Copyright © 2026 Johannes Asal

Sorobo ist freie Software unter der [GNU Affero General Public License, Version 3 oder
später](LICENSE) (AGPL-3.0-or-later). Sie dürfen Sorobo nutzen, untersuchen, verändern und
weitergeben – auch kommerziell. Wer Sorobo weitergibt oder eine veränderte Fassung als Dienst
über ein Netzwerk anbietet, muss den Quellcode dieser Fassung unter derselben Lizenz zugänglich
machen.

Mit Sorobo erstellte Rechnungen, Berichte und sonstige Dokumente gehören Ihnen und unterliegen
nicht der AGPL.

Sorobo enthält Bibliotheken und Schriften anderer Projekte unter deren eigenen freien Lizenzen
(u. a. MIT, Apache-2.0, BSD, SIL Open Font License). Die Lizenzen der Schriften liegen in
[`backend/assets/fonts/LICENSE-FONTS.txt`](backend/assets/fonts/LICENSE-FONTS.txt); die
vollständige Übersicht aller Komponenten erzeugt der Build als `dist/THIRD-PARTY-LICENSES.txt`
(`node scripts/third-party-licenses.mjs`).

## Hinweis

Die Software unterstützt bei der Buchführung, ersetzt aber keine steuerliche Beratung und wird
ohne Gewährleistung bereitgestellt. Die Übermittlung der Umsatzsteuer-Voranmeldung erfolgt über
ELSTER; Sorobo liefert die Kennzahlen.
