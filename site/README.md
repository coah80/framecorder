# the framecorder website

one static page, no build step. open `index.html` through any web server, or put the folder on github pages / cloudflare pages / anything that serves files.

```sh
python3 -m http.server 8080 -d site      # http://localhost:8080
```

before publishing: set `REPO` at the top of `js/main.js` to where the project lives. the source link and the build commands use it.

`install` is the headset installer the page tells people to pipe into `sh`. it downloads `dl/framecorder-arm64.tar.gz`, which the site workflow copies from the latest release (`packaging/release.sh` makes it).

- `js/headset.js`: loads the headset from `model/` and gives its parts their materials. it's valve's own steam frame cad with the insides taken out (cc by-nc-sa 4.0, see `model/LICENSE.txt`, and keep the credit in the footer). `tools/frame_model.py` makes the glb from the step files
- `vendor/GLTFLoader.js`, `vendor/BufferGeometryUtils.js`, `vendor/meshopt_decoder.module.js`: three.js's glb loader and the meshopt decoder it needs, MIT
- `js/main.js`: the hero scene and the things orbiting it (they're plain html, moved around by the 3d scene, so they stay sharp and go behind the headset)
- `css/style.css`: catppuccin mocha, same look as the app and the dashboard tab
- `vendor/three.module.min.js`: three.js r160, MIT
- `img/`: screenshots of the tab and the app. regenerate them with `framecorder-ui --preview` and `app/tools/preview/shots.sh`
