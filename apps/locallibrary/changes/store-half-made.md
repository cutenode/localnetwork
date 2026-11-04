### Fixed

- A store whose making was cut off (a crash after its folders and before `store.json`) is made
  the next time it's opened, where every later command refused its folder as not a store. A
  `store copy` cut off at the same point carries on the next time too.
