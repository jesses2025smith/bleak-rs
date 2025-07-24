## Overview

This project is built on top of the `btleplug` library for Bluetooth Low Energy functionality.

## Dependencies

This project depends on `btleplug` for Bluetooth Low Energy operations. 

Please check the [btleplug issues page](https://github.com/deviceplug/btleplug/issues) 
for potential solutions and known problems if you encounter any issues during usage.

## Contributing

Please note that the author does not accept feature requests or new functionality additions to this project. 

The only updates that will be considered are those related to underlying API changes and maintenance.

## Example

```python
import asyncio
from bleak_py import find_device_by_name, BLEDevice, BLEClient


async def main():
    device: BLEDevice = await find_device_by_name("M1(BLE)", timeout = 10)
    client = BLEClient(device, disconnected_callback=lambda x: print(f"device: {x} disconnected"))
    print(client.address())
    await client.connect()
    await client.start_notify("00002bb0-0000-1000-8000-00805f9b34fb", lambda data: print(f"Received: {data}"))
    val = await client.read_gatt_char("00002a00-0000-1000-8000-00805f9b34fb")
    print(val)
    await client.write_gatt_char("00002bb0-0000-1000-8000-00805f9b34fb", [1, 2, 3, 4])
    await client.disconnect()


if __name__ == "__main__":
    asyncio.run(main())
```
