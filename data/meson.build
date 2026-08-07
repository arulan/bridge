subdir('resources')
subdir('icons')

# app_id has the .Devel suffix
# base_path keeps the GSettings path on base id
conf = configuration_data()
conf.set('app_id', application_id)
conf.set('base_path', base_path)

configure_file(
  input: 'io.github.arulan.Bridge.desktop.in',
  output: application_id + '.desktop',
  configuration: conf,
  install: true,
  install_dir: datadir / 'applications',
)

configure_file(
  input: 'io.github.arulan.Bridge.gschema.xml.in',
  output: application_id + '.gschema.xml',
  configuration: conf,
  install: true,
  install_dir: datadir / 'glib-2.0' / 'schemas',
)

configure_file(
  input: 'io.github.arulan.Bridge.metainfo.xml.in',
  output: application_id + '.metainfo.xml',
  configuration: conf,
  install: true,
  install_dir: datadir / 'metainfo',
)
