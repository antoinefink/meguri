# Third-party notices

Meguri includes the following third-party software. U16 will generate the dependency sections of this file from the shipped dependency graph; the vendored assets below are not in that graph and stay listed here by hand.

## Vendored web assets

### jQuery 3.7.1

- Used in: the card page (`app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/vendor/jquery-3.7.1.min.js`), which card templates call as `$` and `jQuery` (R514).
- Source: the upstream release `jquery-3.7.1.min.js`, unmodified (SHA-256 `fc9a93dd241f6b045cbff0481cf4e1901becd0e12fb45166a8f17f95823f0b1a`, independently verified against the official distribution). It is not the copy Anki ships (R1960).
- License: MIT.

The complete, unmodified release notice is in [jquery-3.7.1-LICENSE.txt](specs/licenses/jquery-3.7.1-LICENSE.txt). Include it with the bundled library in shipped acknowledgements.

### MathJax 4.1.3

The card renderer ships the TeX-to-SVG engine, TeX extensions, assistive MathML
and SVG glyph data for offline study. The bundled JavaScript files are unmodified
members of the upstream npm packages `mathjax`, `@mathjax/mathjax-newcm-font`,
and the `@mathjax/mathjax-{bbm,bboldx,dsfont,mhchem}-font-extension` packages,
all at version 4.1.3. See the [bundled credits and licenses](app/Packages/MeguriKit/Sources/CardRendering/Resources/CardRuntime/vendor/mathjax/NOTICE.txt).
