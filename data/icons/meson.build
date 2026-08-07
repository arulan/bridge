if get_option('profile') == 'development'
  scalable_icon = 'hicolor/scalable/apps/io.github.arulan.Bridge.Devel.svg'
else
  scalable_icon = 'hicolor/scalable/apps/io.github.arulan.Bridge.svg'
endif

install_data(
  scalable_icon,
  install_dir: datadir / 'icons' / 'hicolor' / 'scalable' / 'apps',
  rename: application_id + '.svg',
)

install_data(
  'hicolor/symbolic/apps/io.github.arulan.Bridge-symbolic.svg',
  install_dir: datadir / 'icons' / 'hicolor' / 'symbolic' / 'apps',
  rename: application_id + '-symbolic.svg',
)
