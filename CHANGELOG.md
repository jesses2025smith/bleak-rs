Version 0.1.20
=============

Features
--------
* refactor branch

Version 0.1.16
=============

Features
--------
* add `tx_power_level` `manufacturer_data` `service_data` `services` for `BLEDevice`

Version 0.1.12
=============

Features
--------
 * change `discover` to `async for`

now:
```python
async def _discover():
    async for dev in await discover():
        print(dev.address())
```

before:
```python
async def _discover():
    devices = await discover():
    for dev in devices:
        print(dev.address())
```

Version 0.1.7
=============

Features
--------
* add `characteristic` uuid for `BLEDevice.start_notify` callback

Version 0.1.6
=============

Features
--------
* add `adapter_index` for `discover` | `find_device_by_address` | `find_device_by_name`

Version 0.1.5
=============

Features
--------
* add `characteristic` for `BLEDevice.stop_notify`
