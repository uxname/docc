# schema: отличия Rust от Go

### 1. Другое пояснение, почему файл схемы не принят
Было: если схема вида в `docs/schemas` написана с ошибкой, например `"type": "nonsense"`, Go писал всю цепочку проверки: `docs/schemas/k.json: config error: cannot compile schema: "docc://s/schemas/k.json#" is not valid against metaschema: jsonschema validation failed with 'https://json-schema.org/draft/2020-12/schema#' - at '': 'allOf' failed - at '/type': 'anyOf' failed - at '/type': value must be one of 'array', 'boolean', ... - at '/type': got string, want array`. Ссылку с испорченным `%` (`"$ref": "#%zz"`) Go тоже объяснял через эту цепочку: `... at '/properties/a/$ref': '#%zz' is not valid uri-reference: parse "#%zz": invalid URL escape "%zz"`.
Стало: Rust пишет тот же файл и то же начало `cannot compile schema:`, но причину короче и своими словами: `"docc://s/schemas/k.json#" is not valid against metaschema: at '/type': "nonsense" is not valid under any of the schemas listed in the 'anyOf' keyword`. Про ссылку с испорченным `%` он пишет `error in parsing "#%zz": invalid URL escape "%zz"`. Сообщения о ссылках на несуществующие файлы и места в файлах, о ссылках за пределы папки схем, о недопустимых полях схемы вида и все замечания к самим документам совпадают с Go дословно.
Почему: эти пояснения формулирует библиотека проверки схем, а в Rust она другая и сообщает одну причину, а не всю цепочку. Какой файл с ошибкой и что это ошибка, видно по-прежнему.

### 2. Другое пояснение к части ошибок в регулярных выражениях схем
Было: для неправильного выражения в `pattern` схемы, в имени внутри `patternProperties` или в поле с `"format": "regex"`, например `(`, Go писал `'(' is not valid regex: error parsing regexp: missing closing ): ` + "`(`".
Стало: Rust пишет то же начало `'(' is not valid regex:`, а причину своими словами: `unclosed group`. Для части ошибок (например, `(?<=a)` или `a\1`) текст совпадает с Go дословно.
Почему: так же, как в config (отличие 1): причину большинства ошибок формулирует библиотека регулярных выражений, а в Rust она другая.

### 3. Несколько редких записей в регулярных выражениях схем принимаются иначе
Было: Go не принимал схему с `pattern` или именем в `patternProperties` вида `a{1001}` (повтор больше 1000 раз) или `(?x) a` и принимал `\x{D800}`, которое ничего не находит.
Стало: Rust принимает `a{1001}` и `(?x) a` и проверяет по ним текст, а схему с `\x{D800}` не принимает: `'\\x{D800}' is not valid regex: hexadecimal literal is not a Unicode scalar value`. Все обычные записи (`\d`, `\w`, `\s`, `\b`, `[[:alpha:]]`, `\pL`, `(?i)`, `\Q...\E`, `(?P<n>x)` и другие) и в `pattern`, и в `patternProperties` работают как в Go.
Почему: то же, что отличия 2 и 3 в config: так устроена библиотека регулярных выражений Rust; в схемах документации такие записи не встречаются.

### 4. «unevaluatedProperties» рядом с другой ошибкой: другой набор сообщений «false schema»
Было: схема поля `{"$ref": "lib/e.json#/$defs/e", "unevaluatedProperties": false}`, где в `lib/e.json` написано `{"$defs": {"e": {"additionalProperties": false}}}`, и документ `"v": {"a": ""}`. Go сообщал две ошибки: `/v: property "a" is not allowed` и `/v/a: false schema`. А для `{"oneOf": [{"properties": {"a": {}}}, {"properties": {"b": {}}}], "unevaluatedProperties": false}` и `"v": {"a": 1, "c": 2}` Go сообщал `/v: value does not match any allowed shape` и `/v/c: false schema`.
Стало: в первом примере Rust сообщает только `/v: property "a" is not allowed`; во втором — ещё и `/v/a: false schema`. Ошибочным поле считается в тех же случаях, что и в Go.
Почему: какие поля считать «уже проверенными», когда рядом другая часть схемы не прошла, библиотеки Go и Rust решают по-разному, а повторить правило Go значит написать проверку схем заново. Разница бывает только там, где в том же месте документа уже есть другая ошибка.

### 5. Слишком большое число в документе проверяется как самое большое обычное число
Было: число больше любого допустимого, например `"order": 1e400`, Go отдавал на проверку как «бесконечность» и для любого правила схемы, кроме пустого `{}` и `true`, сообщал одно: `/order: invalid jsonType float64`.
Стало: Rust проверяет его как самое большое обычное число (около `1.8e308`, см. отличие 1 в loader): `"type": "number"` его пропускает, а `"maximum": 10` сообщает `/order: maximum: got 1.7976931348623157 × 10³⁰⁸, want 10`. Так что отличие 1 в loader неточно: для проверки по схеме такое число ведёт себя не совсем так же.
Почему: в формате значений, с которым работает проверка схем в Rust, «бесконечность» записать нельзя. В настоящих документах таких чисел не бывает.
