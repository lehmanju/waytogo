# TODO
- array (de)serialization
- create/destroy wayland objects
    - register/unregister channels
    - if wayland destroys an object, just close stream and throw error on request send
    - if rust drops an object, send message on drop