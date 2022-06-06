# TODO
- array (de)serialization
- create/destroy wayland objects
    - register/unregister channels
    - if wayland destroys an object, just close stream and throw error on request send
    - if rust drops an object, send message on drop

- destroy object itself from rust:
    - send destroy message
    - remove object from socket lists
    - drop object

- destroy some other object from wayland:
    - close channels
    - remove object from socket list

- macro arguments
    - xml file
    - (optional) destroy request -> don't generate request struct but destroy() impl for WaylandInterface

- manual impl for display and registry