use bleasy::{Characteristic, Device, Filter, ScanConfig, Scanner};
use pyo3::{
    exceptions::{PyRuntimeError, PyStopAsyncIteration, PyValueError},
    prelude::PyAnyMethods,
    pyclass, pyfunction, pymethods,
    types::*,
    Bound, PyObject, PyRef, PyRefMut, PyResult, Python,
};
use std::{pin::Pin, sync::Arc, time::Duration};
use stream_cancel::Valved;
use tokio::sync::Mutex;
use tokio_stream::{Stream, StreamExt};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
struct Context {
    notify_characters: Vec<Characteristic>,
}

impl Context {
    #[inline(always)]
    fn push(&mut self, c: Characteristic) {
        self.notify_characters.push(c);
    }

    /// unsubscribe all characters.
    #[inline(always)]
    async fn unsubscribe(&mut self) {
        for c in &self.notify_characters {
            let _ = c.unsubscribe().await;
        }

        self.notify_characters.clear();
    }
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct BLEDevice {
    device: Device,
    context: Arc<Mutex<Context>>,
}

#[pymethods]
impl BLEDevice {
    pub fn address(&self) -> PyResult<String> {
        let address = self.device.address();
        Ok(address.to_string())
    }

    pub fn local_name<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let name = device.local_name().await;

            Ok(name)
        })
    }

    pub fn rssi<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let value = device.rssi().await;

            Ok(value)
        })
    }

    pub fn on_disconnected<'py>(
        &mut self,
        py: Python<'py>,
        callback: PyObject, // Py<PyFunction>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            device.on_disconnected(move |v| {
                Python::with_gil(|py| {
                    if let Err(e) = callback.call1(py, (v.to_string(),)) {
                        e.display(py);
                    }
                })
            });

            Ok(())
        })
    }

    pub fn connect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            device
                .connect()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(())
        })
    }

    pub fn disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();
        let context = self.context.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            context.lock().await.unsubscribe().await;

            device
                .disconnect()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(())
        })
    }

    pub fn is_connected<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let value = device
                .is_connected()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(value)
        })
    }

    pub fn start_notify<'py>(
        &self,
        py: Python<'py>,
        character: Bound<'py, PyString>,
        callback: PyObject, // Py<PyFunction>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let character = character.extract::<&str>()?;
        let uuid = Uuid::try_from(character).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let device = self.device.clone();
        let context = self.context.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let character = device
                .characteristic(uuid)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .ok_or(PyValueError::new_err(format!(
                    "Characteristic not found: {}",
                    uuid
                )))?;

            let mut stream = character
                .subscribe()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Python::with_gil(|py| {
                if let Err(e) = pyo3_async_runtimes::tokio::future_into_py(py, async move {
                    while let Some(data) = stream.next().await {
                        let fut = Python::with_gil(|py| {
                            let uuid = PyString::new(py, &uuid.to_string());
                            let py_data = PyByteArray::new(py, &data);
                            let coroutine = callback.call1(py, (uuid, py_data))?;
                            pyo3_async_runtimes::tokio::into_future(coroutine.extract(py)?)
                        })?;

                        fut.await?;
                    }

                    Ok(())
                }) {
                    e.display(py);
                }
            });

            context.lock().await.push(character);

            Ok(())
        })
    }

    pub fn stop_notify<'py>(
        &self,
        py: Python<'py>,
        character: Bound<'py, PyString>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let character = character.extract::<&str>()?;
        let uuid = Uuid::try_from(character).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let character = device
                .characteristic(uuid)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .ok_or(PyValueError::new_err(format!(
                    "Characteristic not found: {}",
                    uuid
                )))?;

            character
                .unsubscribe()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            // remove from context?

            Ok(())
        })
    }

    pub fn read_gatt_char<'py>(
        &self,
        py: Python<'py>,
        character: Bound<'py, PyString>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let character = character.extract::<&str>()?;
        let uuid = Uuid::try_from(character).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let character = device
                .characteristic(uuid)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .ok_or(PyValueError::new_err(format!(
                    "Characteristic not found: {}",
                    uuid
                )))?;

            let resp = character
                .read()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(resp)
        })
    }

    #[pyo3(signature = (character, data, response = false))]
    pub fn write_gatt_char<'py>(
        &self,
        py: Python<'py>,
        character: Bound<'py, PyString>,
        data: Vec<u8>,
        response: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let character = character.extract::<&str>()?;
        let uuid = Uuid::try_from(character).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let character = device
                .characteristic(uuid)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .ok_or(PyValueError::new_err(format!(
                    "Characteristic not found: {}",
                    uuid
                )))?;
            // let data = data.extract::<Vec<u8>>()?;
            if response {
                character.write_request(&data).await
            } else {
                character.write_command(&data).await
            }
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(())
        })
    }

    pub fn tx_power_level<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let data = device
                .properties()
                .await
                .map(|v| v.tx_power_level)
                .flatten();

            Ok(data)
        })
    }

    #[pyo3(signature = (key))]
    pub fn manufacturer_data<'py>(&self, py: Python<'py>, key: u16) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let data = device.manufacturer_data(&key).await;

            Ok(data)
        })
    }

    #[pyo3(signature = (key))]
    pub fn service_data<'py>(
        &self,
        py: Python<'py>,
        key: Bound<'py, PyString>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let key = key.extract::<&str>()?;
        let uuid = Uuid::try_from(key).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let data = device.service_data(&uuid).await;

            Ok(data)
        })
    }

    pub fn services<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.device.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let data = device.properties().await.map(|v| {
                v.services
                    .into_iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
            });

            Ok(data)
        })
    }
}

#[pyfunction]
#[pyo3(signature = (address, adapter_index = 0, timeout = 15))]
pub fn find_device_by_address<'py>(
    py: Python<'py>,
    address: Bound<'py, PyString>,
    adapter_index: usize,
    timeout: u64,
) -> PyResult<Bound<'py, PyAny>> {
    let address: String = address.extract()?;
    let filters = vec![Filter::Address(address)];
    _find_device(py, filters, adapter_index, timeout)
}

// #[pyfunction]
// #[pyo3(signature = (address, timeout = 15))]
// pub fn find_device_by_rssi<'py>(
//     py: Python<'py>,
//     rssi: i16,
//     timeout: u64,
// ) -> PyResult<Bound<'py, BLEDevice>> {
//     _find_device(py, vec![Filter::Rssi(rssi)], timeout)
// }

#[pyfunction]
#[pyo3(signature = (name, adapter_index = 0, timeout = 15))]
pub fn find_device_by_name<'py>(
    py: Python<'py>,
    name: Bound<'py, PyString>,
    adapter_index: usize,
    timeout: u64,
) -> PyResult<Bound<'py, PyAny>> {
    let name: String = name.extract()?;
    let filters = vec![Filter::Name(name)];
    _find_device(py, filters, adapter_index, timeout)
}

fn _find_device(
    py: Python,
    filters: Vec<Filter>,
    adapter_index: usize,
    timeout: u64,
) -> PyResult<Bound<PyAny>> {
    let duration = Duration::from_secs(timeout);
    let config = ScanConfig::default()
        .adapter_index(adapter_index)
        .with_filters(&filters)
        .stop_after_timeout(duration)
        .stop_after_first_match();
    let mut scanner = Scanner::new();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        scanner
            .start(config)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        while let Some(device) = scanner
            .device_stream()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
            .next()
            .await
        {
            // rsutil::info!("BLE device found: {}", device.address());
            return Ok(BLEDevice {
                device,
                context: Arc::new(Mutex::new(Default::default())),
            });
        }

        Err(PyRuntimeError::new_err(
            bleasy::Error::DeviceNotFound.to_string(),
        ))
    })
}

#[pyfunction]
#[pyo3(signature = (adapter_index = 0, timeout = 15))]
pub fn discover<'py>(
    py: Python<'py>,
    adapter_index: usize,
    timeout: u64,
) -> PyResult<Bound<'py, PyAny>> {
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        let duration = Duration::from_secs(timeout);
        let config = ScanConfig::default()
            .adapter_index(adapter_index)
            .stop_after_timeout(duration);
        let mut scanner = Scanner::new();

        scanner
            .start(config)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let stream = scanner
            .device_stream()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        Ok(DeviceDiscover {
            _scanner: scanner,
            stream: Arc::new(Mutex::new(stream)),
        })
    })
}

#[pyclass]
pub struct DeviceDiscover {
    _scanner: Scanner,
    stream: Arc<Mutex<Valved<Pin<Box<dyn Stream<Item = Device> + Send>>>>>,
}

#[pymethods]
impl DeviceDiscover {
    pub fn __aiter__(this: PyRef<Self>) -> PyRef<Self> {
        this
    }

    pub fn __anext__(this: PyRefMut<Self>) -> PyResult<Bound<PyAny>> {
        let py = this.py();

        let stream = this.stream.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Some(device) = stream.lock().await.next().await {
                Ok(BLEDevice {
                    device,
                    context: Arc::new(Mutex::new(Default::default())),
                })
            } else {
                Err(PyStopAsyncIteration::new_err("End of stream"))
            }
        })
    }
}
