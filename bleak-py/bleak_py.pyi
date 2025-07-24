from typing import Callable, Optional, List

class BLEDevice(object):
    pass

class BLEClient(object):
    def address(self, *args, **kwargs) -> str:
        """Get the address of BLE device"""

    async def connect(self) -> None:
        """
        Connect to the specified GATT server.
        """

    async def disconnect(self) -> None:
        """
        Disconnect from the specified GATT server.
        """

    async def is_connected(self) -> bool:
        """
        Check connection status between this client and the GATT server.

        Returns:
            Boolean representing connection status.

        """

    async def local_name(self) -> Optional[str]:
        """
        The local name of the device or ``None`` if not included in advertising data.
        """

    async def read_gatt_char(self, character: str) -> List[int]:
        """
        Perform read operation on the specified GATT characteristic.

        Args:
            character:
                The characteristic to read from, specified by either integer
                handle, UUID or directly by the BleakGATTCharacteristic object
                representing it.

        Returns:
            The read data.
        """

    async def rssi(self) -> Optional[int]:
        """
        The Radio Receive Signal Strength (RSSI) in dBm.
        """

    async def start_notify(self, character: str, callback: Callable[[List[int], ], None]):
        """
        Activate notifications/indications on a characteristic.

        Callbacks must accept two inputs. The first will be the characteristic
        and the second will be a ``bytearray`` containing the data received.

        Args:
            character:
                The characteristic to activate notifications/indications on a
                characteristic, specified by either integer handle,
                UUID or directly by the BleakGATTCharacteristic object representing it.
            callback:
                The function to be called on notification. Can be regular
                function or async function.
        ."""

    async def stop_notify(self, character: str):
        """
        Deactivate notification/indication on a specified characteristic.

        Args:
            character:
                The characteristic to deactivate notification/indication on,
                specified by either integer handle, UUID or directly by the
                BleakGATTCharacteristic object representing it.
        """

    async def write_gatt_char(self, character: str, data: List[int], response: Optional[bool] = False):
        """
        Perform a write operation on the specified GATT characteristic.

        There are two possible kinds of writes. *Write with response* (sometimes
        called a *Request*) will write the data then wait for a response from
        the remote device. *Write without response* (sometimes called *Command*)
        will queue data to be written and return immediately.

        Each characteristic may support one kind or the other or both or neither.
        Consult the device's documentation or inspect the properties of the
        characteristic to find out which kind of writes are supported.

        Args:
            character:
                The characteristic to write to, specified by either integer
                handle, UUID or directly by the :class:`~bleak.backends.characteristic.BleakGATTCharacteristic`
                object representing it. If a device has more than one characteristic
                with the same UUID, then attempting to use the UUID wil fail and
                a characteristic object must be used instead.
            data:
                The data to send. When a write-with-response operation is used,
                the length of the data is limited to 512 bytes. When a
                write-without-response operation is used, the length of the
                data is limited to :attr:`~bleak.backends.characteristic.BleakGATTCharacteristic.max_write_without_response_size`.
                Any type that supports the buffer protocol can be passed.
            response:
                If ``True``, a write-with-response operation will be used. If
                ``False``, a write-without-response operation will be used.
                Omitting the argument is deprecated and may raise a warning.
                If this arg is omitted, the default behavior is to check the
                characteristic properties to see if the "write" property is
                present. If it is, a write-with-response operation will be
                used. Note: some devices may incorrectly report or omit the
                property, which is why an explicit argument is encouraged.
        """

    def __init__(self, device: BLEDevice, disconnected_callback: Optional[Callable[[str, ], None]] = None):
        pass


async def find_device_by_address(address: str, timeout: Optional[int] = 15) -> BLEDevice:
    """
    Obtain a ``BLEDevice`` for a BLE server that matches the address given.
    """

async def find_device_by_name(name: str, timeout: Optional[int] = 15) -> BLEDevice:
    """
    Obtain a ``BLEDevice`` for a BLE server that matches the name given.
    """
