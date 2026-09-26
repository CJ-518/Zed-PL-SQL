[English](README.md) | [Español](README.es.md)

# Zed-PL-SQL

Extensión de Zed que conecta [plsqllang-server](https://github.com/EwanDubashinski/plsqllang-server) (un LSP de PL/SQL para verificación de sintaxis) a Zed como servidor de lenguaje para archivos SQL.

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

## Instalación de esta extensión

1. Cloná este repositorio.
2. En Zed: Paleta de comandos → `zed: install dev extension` → seleccioná la carpeta clonada.
3. Abrí un archivo `.sql` — deberían aparecer diagnósticos de `plsqllang-server`.

## Limitaciones

- **Solo verifica sintaxis** — no tiene conocimiento semántico de tu esquema de base de datos real (no valida tablas, columnas o paquetes contra una conexión activa).
- **Requiere obtener manualmente `server-all.jar`** según lo descrito arriba (ver Requisitos previos) — el repositorio original `plsqllang-server` actualmente no se puede compilar desde el código fuente de forma independiente.
- **Falsos positivos en sintaxis de scripting de SQL\*Plus / SQLcl.** El parser está orientado específicamente a bloques PL/SQL y no entiende directivas propias del cliente SQL\*Plus. Es de esperar que marque como error líneas válidas como:
  - `SET SERVEROUTPUT ON`, `SET VERIFY OFF`, y otros comandos `SET`
  - Asignaciones `DEFINE variable = valor`
  - Referencias `&variable_de_sustitucion`

  aunque estas se ejecuten sin problemas en SQLcl o SQL\*Plus. Son falsos positivos por el alcance del parser, no errores reales en tu SQL — podés ignorar los diagnósticos en líneas con este tipo de sintaxis propia del cliente.
- **Falso positivo después de la primera sentencia de nivel superior en scripts con múltiples sentencias.** La gramática del parser parece aceptar solo una unidad de nivel superior antes de `EOF`. Un archivo con más de una unidad terminada en `/` (por ejemplo, un bloque `CREATE PROCEDURE ... END; /` seguido de un `CREATE FUNCTION ...`) va a reportar `mismatched input 'CREATE' expecting <EOF>` (o similar) en la segunda unidad en adelante, aunque cada una sea PL/SQL válido por sí sola. Esta es una limitación del parser original, no algo que esta extensión pueda evitar — `language_server_command` solo controla cómo se lanza el proceso del servidor, no cómo se fragmenta el buffer antes de llegar a él. Podés ignorar diagnósticos de este tipo en sentencias posteriores dentro de un script; considerá reportarlo en el proyecto original `plsqllang-server`/parser.
- Proyecto original mantenido por una sola persona y sin releases — es esperable encontrar asperezas ocasionales en el parser subyacente, independientes de esta extensión.
