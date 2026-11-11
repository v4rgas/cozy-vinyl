# Rust y audio del computador

Investigación: 11 de noviembre de 2026. Objetivo: conservar el tocadiscos, sus ecuaciones y controles en una aplicación nativa Rust; investigar la reproducción de otras aplicaciones a través de la simulación. Primera plataforma asumida: Linux, por el entorno de desarrollo. Widget y fondo de pantalla quedan fuera de esta etapa.

## Librerías y decisión

- [eframe 0.33](https://docs.rs/eframe/0.33.0/eframe/) y glow permiten una interfaz nativa y dibujar OpenGL dentro de ella. El [ejemplo oficial custom3d_glow](https://github.com/emilk/egui/blob/0.33.0/crates/egui_demo_app/src/apps/custom3d_glow.rs), leído como referencia, conserva los recursos de GPU entre cuadros y pinta mediante un callback. Adoptamos ese patrón para portar los shaders existentes y evitar rediseñar el tocadiscos.
- [CPAL 0.16](https://docs.rs/cpal/0.16.0/cpal/) proporciona dispositivos y callbacks de audio nativos. El motor físico vive fuera del hilo gráfico; los controles viajan por una cola acotada. La decodificación y el prensado se hacen fuera del callback.
- [Symphonia](https://github.com/pdeljanov/Symphonia) decodifica audio en Rust, incluidos FLAC, MP3 y Vorbis. Los FLAC de `assets/` contienen diferencias de desplazamiento del surco, no música lista para reproducir: deben reconstruirse sin remuestrear ni normalizar.
- No usamos un motor general de cuerpos rígidos: no reemplaza el integrador específico de contacto aguja/surco ni la simulación eléctrica de la cápsula. Se portan las ecuaciones existentes.

Se consideraron Tauri (conservaría UI JavaScript) y una escena nueva en wgpu (requeriría migrar los shaders a WGSL). Para esta reescritura completa del runtime en Rust, glow permite conservar más directamente la estética y el sombreado existentes. GLSL sigue siendo el lenguaje de los shaders de GPU.

## Spotify: metadatos y sonido son rutas diferentes

La [Web API de reproducción actual](https://developer.spotify.com/documentation/web-api/reference/get-the-users-currently-playing-track) permite consultar la canción y el álbum con autorización del usuario. No entrega el PCM de la canción. El [Web Playback SDK](https://developer.spotify.com/documentation/web-playback-sdk/reference) reproduce contenido en el navegador, pero su interfaz no expone un callback de muestras para insertar nuestro motor; además, la documentación de la plataforma exige mantener el contenido de audio en su forma original. Por eso no se propone el SDK como entrada de nuestro procesador.

Una integración futura debe separar:

1. **Metadatos:** artista, título, duración y carátula, obtenidos mediante OAuth/API o los controles multimedia del sistema cuando estén disponibles. La carátula no implica acceso al audio.
2. **Entrada de audio:** una ruta de captura del sistema elegida por la persona, independiente de las credenciales de Spotify.
3. **Motor en directo:** conversión causal de PCM a un surco circular limitado, lectura física y salida. El torno actual prepara un disco completo; no basta conectar un micrófono a su API.

Esto describe viabilidad técnica, no una integración aprobada por Spotify. Las condiciones aplicables al producto y a las fuentes de audio deben revisarse antes de distribuir una integración comercial.

## Captura por plataforma

| Plataforma | Mecanismo investigado | Implicación |
| --- | --- | --- |
| Linux | [PipeWire: loopback y dispositivos virtuales](https://pipewire.pages.freedesktop.org/pipewire/page_module_loopback.html) | Enrutar la app fuente a una salida virtual, capturar esa ruta y enviar nuestro resultado a la salida física. El módulo loopback por sí mismo no aplica el efecto. |
| Windows | [WASAPI loopback](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording) | Captura la mezcla de un endpoint en modo compartido; la selección de una sola aplicación requiere investigar captura por proceso o un dispositivo virtual. |
| macOS | [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos) | Candidato para captura de audio del sistema, sujeto a permisos y versión. Su implementación Rust queda pendiente de un prototipo en macOS. |

No capturar el mismo dispositivo al que enviamos el resultado: produciría realimentación. El diseño necesita relojes de entrada/salida independientes, búfer acotado, corrección de deriva, manejo de underruns y una ruta de bypass. La captura no debe grabar ni conservar canciones en disco por defecto.

## Diferencias inevitables con un disco ya grabado

Un flujo en vivo no permite leer audio futuro. Frenar el disco acumula retraso; acelerarlo consume el búfer. Al agotar datos hace falta una política explícita de silencio, resincronización o reproducción diferida. Arrastrar la aguja a una canción que todavía no llegó no puede funcionar igual que en un LP completo. Debe conservarse un modo de disco local y diseñarse un modo en directo con estos límites visibles.

La siguiente implementación recomendada es un prototipo Linux con una fuente PCM propia o libre, una salida virtual PipeWire y medición de latencia. Después se puede añadir sincronización de metadatos y carátulas. El procesamiento en directo y la autenticación de Spotify no se presentan como implementados en esta reescritura.
