{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  packages = with pkgs; [
	rustc
	cargo
	pkg-config
	patchelf
	imagemagick
	gtk3
	libx11
	libxext
	libxrender
	libxcb
	wayland
	mesa
	vulkan-loader
  ];
}