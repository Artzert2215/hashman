{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  packages = with pkgs; [
	rustc
	cargo
	pkg-config
	patchelf
	imagemagick
	libx11
	libxext
	libxrender
	libxcb
	wayland
	mesa
	vulkan-loader
	rustfmt
	clippy
  ];
}