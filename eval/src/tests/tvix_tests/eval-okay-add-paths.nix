[
  (/bin + "/")
  (/bin + "bar")
  (let name = "bar"; in /bin + name)
  (let name = "bar"; in /bin + "${name}")
  (let name = "bar"; in /bin + "/" + "${name}")
  (let name = "bar"; in /bin + "/${name}")
  (/bin + /bin)
]
