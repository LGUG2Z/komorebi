# What komorebi-recorder needs of the system on Linux: the packages and libraries, for its build, its
# wrapper and the dev shell.
{ pkgs }:
rec {
  # what GStreamer needs: the libraries to link, and the plugins that decode (libav, for H.264 and
  # HEVC) and encode (x264, in ugly) the recorded video
  gstreamerPackages = with pkgs.gst_all_1; [
    gstreamer
    gst-plugins-base
    gst-plugins-good
    gst-plugins-bad
    gst-plugins-ugly
    gst-libav
    pkgs.pipewire
  ];

  gstreamerPluginPath = pkgs.lib.makeSearchPathOutput "lib" "lib/gstreamer-1.0" gstreamerPackages;

  # the libraries the program opens when it needs them: Wayland, for the compositor and the window,
  # and what the window draws through
  linuxRuntimeLibraries = [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.libGL
    pkgs.vulkan-loader
    pkgs.libx11
    pkgs.libxcursor
    pkgs.libxi
    pkgs.libxrandr
  ];
}
