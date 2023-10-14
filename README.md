# vrr - very rapid renderer

Vrr is a simple and fast renderer for images.

It is inspired by tools like [feh](https://feh.finalrewind.org/)
and [gthumb](https://wiki.gnome.org/Apps/Gthumb), but aims to be faster
and smoother when working with large images and image directories.

Vrr is written in Rust and uses WebGPU for rendering.

JPEG go vrr!

## Features

- Preload images in proximity to current image, enabling quick display when flipping through directory

## Usage

- `j` - next image
- `k` - previous image
- `f` - toggle fullscreen
- `x` - reset view
- `m` - mark image as favorite
- `q` - quit


## TODO

- [ ] Load preview images
- [ ] Add support for more image formats
- [ ] Make more use of wsgl - edge detection, etc.
- [ ] add local image cache
- [ ] add config file
- [ ] add scripting support

## Credits

Based on some code from [learn-wgpu](https://github.com/sotrh/learn-wgpu).
