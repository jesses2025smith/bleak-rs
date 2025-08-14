from typing import Callable, Coroutine, Optional, List, Union

class BLEDevice(object):
    def address(self) -> str:
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

    def on_disconnected(self, callback: Callable[[str, ], None]):
        """
        The callback when device is disconnected.
        """

    async def read_gatt_char(self, character: str) -> List[int]:
        """
        Perform read operation on the specified GATT characteristic.

        Args:
            character:
                The characteristic(a uuid string) to read from.

        Returns:
            The read data.
        """

    async def rssi(self) -> Optional[int]:
        """
        The Radio Receive Signal Strength (RSSI) in dBm.
        """

    async def start_notify(self, character: str, callback: Callable[[str, bytearray, ], Coroutine[Any, Any, None]]):
        """
        Activate notifications/indications on a characteristic.

        Callbacks must accept two inputs. The first will be the characteristic
        and the second will be a ``bytearray`` containing the data received.

        Args:
            character:
                The characteristic(a uuid string) to activate
                notifications/indications on a characteristic.
            callback:
                The function to be called on notification. Can be regular
                function or async function.
        ."""

    async def stop_notify(self, character: str):
        """
        Deactivate notification/indication on a specified characteristic.

        Args:
            character:
                The characteristic(a uuid string) to deactivate
                notifications/indications on a characteristic.
        """

    async def write_gatt_char(self, character: str, data: Union[bytes, bytearray, List[int]], response: bool = False):
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
                The characteristic(a uuid string) to write to.
            data:
                The data to send.
            response:
                If ``True``, a write-with-response operation will be used. If
                ``False``, a write-without-response operation will be used.
        """

    async def tx_power_level(self) -> Optional[int]:
        """Get the tx power from device"""

    async def manufacturer_data(self, key: int) -> Optional[bytearray]:
        """Get the manufacturer data from device"""

    async def service_data(self, key: str) -> Optional[bytearray]:
        """Get the service data device"""

    async def services(self) -> Optional[list[str]]:
        """Get the services from device"""


class DeviceDiscover:
    async def __aiter__(self):
        pass

    async def __anext__(self) -> BLEDevice:
        pass


async def discover(adapter_index: int = 0, timeout: int = 15) -> DeviceDiscover:
    """
    Obtain ``BLEDevice``s for a BLE server in during time.
    """


async def find_device_by_address(address: str, adapter_index: int = 0, timeout: int = 15) -> BLEDevice:
    """
    Obtain a ``BLEDevice`` for a BLE server that matches the address given.
    """

async def find_device_by_name(name: str, adapter_index: int = 0, timeout: int = 15) -> BLEDevice:
    """
    Obtain a ``BLEDevice`` for a BLE server that matches the name given.
    """
