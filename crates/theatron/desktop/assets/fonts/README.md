# Font Acquisition

The desktop design system uses two typefaces, both licensed under the SIL Open Font License (OFL):

- **IBM Plex Mono** — code and precision text
- **Cormorant Garamond** — display headings

## Download

Run the acquisition script from the desktop crate root:

```bash
./scripts/fetch-fonts.sh
```

Or download manually:

1. **IBM Plex Mono** (Regular, Medium, SemiBold, Bold + italic variants):
   https://github.com/IBM/plex/releases — download `IBM-Plex-Mono.zip`

2. **Cormorant Garamond** (Regular, Medium, SemiBold, Bold + italic variants):
   https://github.com/CatharsisFonts/Cormorant/releases — download the latest release

Place `.woff2` files in this directory. The `@font-face` declarations in
`styles/fonts.css` reference these filenames:

```
IBMPlexMono-Regular.woff2
IBMPlexMono-Medium.woff2
IBMPlexMono-SemiBold.woff2
IBMPlexMono-Bold.woff2
IBMPlexMono-Italic.woff2
CormorantGaramond-Regular.woff2
CormorantGaramond-Medium.woff2
CormorantGaramond-SemiBold.woff2
CormorantGaramond-Bold.woff2
CormorantGaramond-Italic.woff2
```

## Licensing

Both fonts are SIL OFL 1.1 — free to use, embed, and redistribute.
