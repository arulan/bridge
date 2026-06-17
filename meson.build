project('dashboard', 'rust',
  version: '0.3.0',
  meson_version: '>= 1.0',
  default_options: ['warning_level=2'],
)

gnome = import('gnome')

application_id = 'io.github.arulan.Dashboard'

prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
datadir = prefix / get_option('datadir')
pkgdatadir = datadir / meson.project_name()

subdir('data')
subdir('src')

gnome.post_install(
  glib_compile_schemas: true,
  gtk_update_icon_cache: true,
  update_desktop_database: true,
)
