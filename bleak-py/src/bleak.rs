use bleasy::{Device, Filter, ScanConfig, Scanner};
use pyo3::{
    Bound, Py, PyResult, Python, exceptions::PyRuntimeError, prelude::PyAnyMethods, pyclass,
    pyfunction, pymethods, types::*,
};
use std::{sync::{mpsc::channel, Arc}, time::Duration, thread};
use tokio::{spawn, sync::Mutex, task::JoinHandle, time};
use tokio_stream::StreamExt;
use uuid::Uuid;

#[derive(Debug)]
struct Context {
    notify_character: Uuid,
    subscribe_task: JoinHandle<()>,
}

impl Context {
    async fn unsubscribe(&self, device: &Device) {
        if let Ok(Some(char)) = device.characteristic(self.notify_character).await {
            let _ = char.unsubscribe().await;
        }

        self.subscribe_task.abort();
    }
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct BLEDevice {
    device: Device,
    context: Arc<Mutex<Option<Context>>>,
}

#[pyclass]
#[derive(Debug, Clone)]
pub struct BLEClient {
    inner: BLEDevice,
}

#[pymethods]
impl BLEClient {
    #[new]
    #[pyo3(signature = (device, disconnected_callback=None))]
    pub fn new(
        mut device: BLEDevice,
        disconnected_callback: Option<Py<PyFunction>>,
    ) -> Self {
        let (tx, rx) = channel::<String>();

        if let Some(callback) = disconnected_callback {
            thread::spawn(move || {
                // Receive messages from the channel
                while let Ok(value) = rx.recv() {
                    Python::with_gil(|py| {
                        if let Err(e) = callback.call1(py, (value, )) {
                            e.display(py);
                        }
                    });
                }
            });
        }

        // Register the on_disconnected callback with a Send + 'static closure
        device.device.on_disconnected(move |v| {
            // Send the PeripheralId through the channel
            let _ = tx.send(v.to_string());
        });

        Self { inner: device }
    }

    pub fn address(&self) -> PyResult<String> {
        let address = self.inner.device.address();
        Ok(address.to_string())
    }

    pub fn local_name<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.inner.device.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let name = device.local_name().await;

            Ok(name)
        })
    }

    pub fn rssi<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.inner.device.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let value = device.rssi().await;

            Ok(value)
        })
    }

    pub fn connect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.inner.device.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            device
                .connect()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(())
        })
    }

    pub fn disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.inner.device.clone();
        let context = self.inner.context.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Some(ctx) = context.lock().await.take() {
                ctx.unsubscribe(&device).await;
            }

            device
                .disconnect()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            Ok(())
        })
    }

    pub fn is_connected<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.inner.device.clone();
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
        callback: Py<PyFunction>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let uuid = character.extract::<Uuid>()?;
        let device = self.inner.device.clone();
        let context = self.inner.context.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let character = device
                .characteristic(uuid)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .ok_or(PyRuntimeError::new_err(format!(
                    "Characteristic not found: {}",
                    uuid
                )))?;

            let mut stream = character
                .subscribe()
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            let handle = spawn(async move {
                while let Some(data) = stream.next().await {
                    // rsutil::trace!("Received: {}", hex::encode(&data));
                    Python::with_gil(|py| {
                        let py_data = PyBytes::new(py, &data);
                        if let Err(e) = callback.call1(py, (py_data,)) {
                            e.display(py);
                        }
                    });
                }
            });

            context.lock().await.replace(Context {
                notify_character: uuid,
                subscribe_task: handle,
            });

            Ok(())
        })
    }

    pub fn stop_notify<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let device = self.inner.device.clone();
        let context = self.inner.context.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Some(ctx) = context.lock().await.take() {
                ctx.unsubscribe(&device).await;
            }

            Ok(())
        })
    }

    pub fn read_gatt_char<'py>(
        &self,
        py: Python<'py>,
        character: Bound<'py, PyString>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let device = self.inner.device.clone();
        let uuid = character.extract::<Uuid>()?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let character = device
                .characteristic(uuid)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .ok_or(PyRuntimeError::new_err(format!(
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
        let device = self.inner.device.clone();
        // let context = self.inner.context.clone();
        let uuid = character.extract::<Uuid>()?;
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let character = device
                .characteristic(uuid)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .ok_or(PyRuntimeError::new_err(format!(
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
#[pyo3(signature = (address, timeout = 15))]
pub fn find_device_by_address<'py>(
    py: Python<'py>,
    address: Bound<'py, PyString>,
    timeout: u64,
) -> PyResult<Bound<'py, PyAny>> {
    let address: String = address.extract()?;
    let filters = vec![Filter::Address(address)];
    _find_device(py, filters, timeout)
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
#[pyo3(signature = (name, timeout = 15))]
pub fn find_device_by_name<'py>(
    py: Python<'py>,
    name: Bound<'py, PyString>,
    timeout: u64,
) -> PyResult<Bound<'py, PyAny>> {
    let name: String = name.extract()?;
    let filters = vec![Filter::Name(name)];
    _find_device(py, filters, timeout)
}

fn _find_device(py: Python, filters: Vec<Filter>, timeout: u64) -> PyResult<Bound<PyAny>> {
    let duration = Duration::from_secs(timeout);
    let config = ScanConfig::default()
        .with_filters(&filters)
        .stop_after_timeout(duration.clone())
        .stop_after_first_match();
    let mut scanner = Scanner::new();

    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        scanner
            .start(config)
            .await
            .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

        time::timeout(duration, async move {
            while let Some(device) = scanner
                .device_stream()
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?
                .next()
                .await
            {
                // rsutil::info!("BLE device found: {}", device.address());
                return Ok(BLEDevice {
                    device,
                    context: Arc::new(Mutex::new(None)),
                });
            }

            Err(PyRuntimeError::new_err(
                bleasy::Error::DeviceNotFound.to_string(),
            ))
        })
        .await
        .map_err(|_| {
            PyRuntimeError::new_err(format!(
                "Couldn't find the device after time elapsed: {} seconds",
                timeout
            ))
        })?
    })
}
