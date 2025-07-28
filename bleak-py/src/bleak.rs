use bleasy::{Device, Filter, ScanConfig, Scanner};
use pyo3::{
    exceptions::{PyRuntimeError, PyValueError},
    prelude::PyAnyMethods,
    pyclass, pyfunction, pymethods,
    types::*,
    Bound, PyObject, PyResult, Python,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_stream::StreamExt;
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
struct Context {
    notify_characters: Vec<Uuid>,
}

impl Context {
    #[inline(always)]
    fn push(&mut self, uuid: Uuid) {
        self.notify_characters.push(uuid);
    }

    /// unsubscribe all characters.
    #[inline(always)]
    async fn unsubscribe(&mut self, device: &Device) {
        for uuid in &self.notify_characters {
            if let Ok(Some(char)) = device.characteristic(uuid.clone()).await {
                let _ = char.unsubscribe().await;
            }
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
            context.lock().await.unsubscribe(&device).await;

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
                            let py_data = PyByteArray::new(py, &data);
                            let coroutine = callback.call1(py, (py_data,))?;
                            pyo3_async_runtimes::tokio::into_future(coroutine.extract(py)?)
                        })?;

                        fut.await?;
                    }

                    Ok(())
                }) {
                    e.display(py);
                }
            });

            context.lock().await.push(uuid);

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
}

#[pyfunction]
#[pyo3(signature = (adapter_index = 0, timeout = 15))]
pub fn discover(py: Python, adapter_index: usize, timeout: u64) -> PyResult<Bound<PyAny>> {
    let duration = Duration::from_secs(timeout);
    let config = ScanConfig::default()
        .adapter_index(adapter_index)
        .stop_after_timeout(duration);
    let mut scanner = Scanner::new();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        scanner
            .start(config)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        let mut results = Vec::new();
        while let Some(device) = scanner
            .device_stream()
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
            .next()
            .await
        {
            results.push(BLEDevice {
                device,
                context: Arc::new(Mutex::new(Default::default())),
            });
        }

        Ok(results)
    })
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

fn _find_device(py: Python, filters: Vec<Filter>, adapter_index: usize, timeout: u64) -> PyResult<Bound<PyAny>> {
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
