# YHK Printer

## La idea

Tenía una impresora térmica que solo funcionaba con su propia app, así que le hicimos ingeniería inversa para crear una TUI para todos.

El dispositivo es una impresora térmica portátil de 57 mm, la YHK-A32B. El objetivo era simple: usarla como una impresora normal desde la computadora, con una herramienta abierta que cualquiera pueda usar, en vez de depender de WalkPrint, la app del proveedor.

## El problema

La impresora no se comportaba como una impresora de tickets estándar. Mandarle texto ESC/POS no hacía nada. Era evidente que podíamos alcanzarla, pero no decía cómo esperaba recibir una página.

Un escaneo de Bluetooth Low Energy nos dio la primera pista: la impresora anunciaba un servicio GATT estilo UART transparente, con una característica de escritura y dos de notificación. Una conexión directa desde una Mac era estable, aunque el propio sistema operativo seguía cortándola. La impresora respondía algunas consultas de estado con respuestas cortas, incluyendo versiones de hardware y firmware, voltaje de batería, ancho de impresión, un serial, un código de error y un ID de producto. Su ancho imprimible es de 384 puntos, unos 48 mm sobre papel de 57 mm.

## Cómo encontramos una entrada

Con el visto bueno del dueño, estudiamos el paquete Android de WalkPrint sin instalarlo ni ejecutarlo. Al decompilarlo aparecieron hechos del protocolo, no código reutilizable del proveedor: la secuencia para densidad, inicialización y datos de bitmap; un segundo formato de bitmap en escala de grises; códigos de error como tapa abierta; y una sorpresa útil. Lo que la app llamaba autorización era apenas una marca local activada después de una comprobación del servidor, no un requisito impuesto por la impresora.

La app usa Bluetooth clásico en Android, mientras que la impresora también habla BLE. Su biblioteca BLE manda bloques de 20 bytes con una pausa de 20 ms. Eso ayudó a establecer un escritor pausado y seguro. No se publica nada de la app —código, claves ni recursos—; solo se usan comportamientos observados del protocolo.

## La pared

Al principio, la impresora aceptaba datos y movía el papel, pero solo producía una línea fina o nada. Probamos y descartamos una explicación por vez: formato de comando equivocado, escritura demasiado rápida, modo de escala de grises y densidad de impresión demasiado baja.

Una escalera de seis bloques sólidos con densidad creciente produjo seis líneas finas idénticas. Fue un buen fracaso: descartó la densidad en vez de sumar otra conjetura.

## El avance

Probamos tres variantes de bitmap. Solo una produjo una barra sólida: mandaba un comando raster separado por cada fila. La conclusión verificada es que esta impresora imprime exactamente una fila de puntos por comando raster e ignora el campo de altura. Cuando mandamos un comando por fila, salió la primera foto real.

El hardware también nos enseñó dos reglas prácticas. Con la tapa abierta, ignora los trabajos silenciosamente e informa el código de error 02. Y solo un programa puede mantener la conexión Bluetooth a la vez.

## Construirlo con agentes

El resultado es una CLI en Rust llamada `yhkp`, con una capa de protocolo pura, una tubería de imagen y un escritor Bluetooth pausado. Tiene alrededor de cien pruebas automatizadas.

Un agente autónomo de programación, OpenCode en modo auto, escribió el código a partir de especificaciones escritas. Claude orquestó y verificó el trabajo de manera independiente: volvió a ejecutar pruebas, clippy y el formateo; renderizó las salidas; y las inspeccionó. Esa pasada independiente encontró defectos que el informe del propio agente no había detectado: faltaban pruebas para la capa de seguridad; una cola nunca reemplazaba la primera imagen cuando llegaba una nueva; un componedor tipográfico dibujaba solo la primera línea de documentos de varias líneas y fallaba con reglas horizontales; el texto salía 2,8 veces demasiado chico porque se trataban puntos como píxeles; una estimación de tiempo de impresión erraba por un factor de nueve; y faltaba el espaciado entre párrafos.

## Qué hace hoy

La interfaz de terminal acepta archivos por arrastrar y soltar, previsualiza imágenes y mantiene una cola ordenada de imágenes con una cuadrícula de miniaturas. Ofrece tres modos por lote: imprimir uno y avanzar, pausar entre impresiones para cortar el papel y rollo continuo.

Tiene niveles de calidad naturales —bajo, medio y alto—, junto con tipos de contenido automático, foto, texto y QR, más una opción de definición HD. Los códigos QR se imprimen pixel-perfect y el código de prueba impreso se pudo escanear correctamente.

Los documentos de texto y Markdown se componen al ancho de la impresora usando tres tipografías de licencia abierta: Atkinson Hyperlegible, Patrick Hand e IBM Plex Mono. Se imprimieron bien. También hay una API HTTP local, limitada a loopback, con comprobaciones de Host y Origin y un token opcional, para que una página web no pueda imprimir sin que vos lo sepas. Un editor en el navegador muestra el bitmap exacto que se va a imprimir e incluye plantillas y un compositor de tarjetas.

El espacio entre impresiones y las marcas de corte punteadas surgieron después de ver fotos impresas de borde a borde. Las tarjetas ponen una foto y un mensaje en las dos caras de una misma pieza. En el modo “detrás”, la impresora produce la foto, te indica que cortes, des vuelta la hoja como una página de libro o calendario y la reinsertes; después imprime el texto en un recuadro exactamente tan alto como la foto. El dueño probó esos dos pasos a mano sobre papel de doble cara, con los mismos bitmaps, y funcionó. La secuencia guiada dentro de la TUI todavía no se ejecutó en la impresora.

El envío es lento a propósito, cerca de 380 bytes por segundo. Una foto de 256 filas lleva aproximadamente 38 segundos y una carta completa, un par de minutos.

## Qué está verificado y qué no

### Verificado en hardware real

Conexión y consultas de estado, impresión fila por fila de fotos, QR y texto, impresión de cartas en Markdown e impresión a mano de las dos caras de una tarjeta (primero el frente, después el dorso reinsertado).

### No verificado

Rollo continuo y modo pausa en papel; si la densidad de impresión realmente cambia la oscuridad (nunca se comparó lado a lado); cómo se ve la opción HD en papel; precisión de alineación al reinsertar a mano (esperá 1 a 2 mm); trabajos muy largos; comportamiento sobre papel de una sola cara (el dorso no se puede imprimir); si el ritmo lento de envío realmente hace falta (sigue la librería del fabricante y nunca se probó más rápido); y el editor del navegador y la secuencia guiada de tarjetas de la TUI, que solo se ejercitaron con pruebas automáticas y llamadas a la API, no a mano en un navegador real ni en la impresora.

## Qué aprendimos

Que las pruebas pasen no demuestra que un proyecto de hardware funcione: hay que mirar la salida y medir. Probá una hipótesis a la vez, con una prueba que pueda fallar. Conservá una línea de base dorada antes de refactorizar. El resumen de un agente es una afirmación que hay que verificar. Y el hardware tiene rarezas que nadie documenta.

## Qué sigue

La próxima funcionalidad solicitada es una vista web que muestre cada elemento cargado en la TUI. El código se está refactorizando según los principios SOLID y KISS, con una red de seguridad exacta a nivel de bytes. Después vienen el empaquetado, un repositorio público en GitHub y una licencia MIT.
