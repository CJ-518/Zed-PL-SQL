[English](README.md) | [Español](README.es.md)

# Zed-PL-SQL

Extensión de Zed que conecta [plsqllang-server](https://github.com/EwanDubashinski/plsqllang-server) (un LSP de PL/SQL para verificación de sintaxis) a Zed como servidor de lenguaje para archivos SQL.

Un pequeño proxy (`plsqllang-proxy`) se ubica entre Zed y el `plsqllang-server` real. Como la gramática del parser original solo verifica una única sentencia de nivel superior por documento, el proxy divide cada buffer en un documento virtual por cada sentencia SQL/PL-SQL de nivel superior, envía cada una al servidor real por separado, y remapea + combina los diagnósticos que recibe de vuelta sobre los números de línea del documento real — así, todas las sentencias de un script con múltiples sentencias quedan verificadas, no solo la primera. El proxy también filtra diagnósticos de falsos positivos conocidos para directivas propias del cliente SQL\*Plus/SQLcl que el parser no entiende.

## Requisitos previos

Necesitás un binario `plsqllang-server` en tu PATH. La forma más fácil de conseguirlo es el
`server-all.jar` incluido dentro del archivo `.vsix` de la [extensión plsqllang-client para VS Code](https://marketplace.visualstudio.com/items?itemName=EwanDubashinski.plsqllint)
(en `extension/server/server-all.jar`) — compilar el repositorio `plsqllang-server`
directamente desde el código fuente falla actualmente, ya que depende de un módulo
`parser` que no está publicado públicamente.

Requiere un JDK (1.8 o superior).

### Windows
Creá `plsqllang-server.bat` en algún lugar de tu PATH:
```bat
@echo off
java -jar "C:\ruta\a\server-all.jar" %*
```

### macOS / Linux
Creá un script ejecutable `plsqllang-server` en algún lugar de tu PATH:
```bash
#!/usr/bin/env bash
exec java -jar /ruta/a/server-all.jar "$@"
```

También necesitás el binario `plsqllang-proxy` en tu PATH. Compilalo desde el directorio `proxy/` de este repositorio (`cargo build --release`) y asegurate de que el binario resultante esté en el PATH.

## Instalación de esta extensión

1. Cloná este repositorio.
2. En Zed: Paleta de comandos → `zed: install dev extension` → seleccioná la carpeta clonada.
3. Abrí un archivo `.sql` — deberían aparecer diagnósticos de `plsqllang-server`, enrutados a través de `plsqllang-proxy`.

## Limitaciones

- **Solo verifica sintaxis** — no tiene conocimiento semántico de tu esquema de base de datos real (no valida tablas, columnas o paquetes contra una conexión activa).
- **Requiere obtener manualmente `server-all.jar`** según lo descrito arriba (ver Requisitos previos) — el repositorio original `plsqllang-server` actualmente no se puede compilar desde el código fuente de forma independiente.
- **Falsos positivos en sintaxis de scripting de SQL\*Plus / SQLcl.** El parser está orientado específicamente a bloques PL/SQL y no entiende directivas propias del cliente SQL\*Plus. `plsqllang-proxy` filtra los casos conocidos — comandos `SET` (`SET SERVEROUTPUT ON`, `SET VERIFY OFF`, etc.), asignaciones `DEFINE variable = valor` y referencias `&variable_de_sustitucion` — pero cualquier directiva que todavía no esté en su lista de filtros puede seguir marcándose como error de sintaxis aunque se ejecute sin problemas en SQLcl o SQL\*Plus. Si te encontrás con uno, es una entrada de filtro faltante, no un error real en tu SQL.
- **Hover / ir a definición puede no resolver correctamente.** `plsqllang-proxy` divide cada documento en documentos virtuales por sentencia para que todas las sentencias queden verificadas (solucionando la limitación original de una sola sentencia para los diagnósticos), pero solicitudes como hover o ir a definición siguen haciendo referencia a posiciones del documento real — posiciones que el servidor subyacente nunca ve directamente, ya que solo opera sobre los documentos virtuales de cada fragmento. Estas solicitudes se reenvían sin modificar y, como resultado, pueden no resolver correctamente.
- Proyecto original mantenido por una sola persona y sin releases — es esperable encontrar asperezas ocasionales en el parser subyacente, independientes de esta extensión.
