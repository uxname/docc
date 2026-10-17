# comments: отличия Rust от Go

### 1. Другой текст ошибки у испорченного файла замечаний
Было: `docc comments import x.json` на файле с ошибкой в JSON печатал текст стандартной библиотеки Go: `x.json: invalid character '}' looking for beginning of object key string`, `x.json: json: cannot unmarshal string into Go struct field Export.version of type int`.
Стало: текст пишет serde_json и указывает строку: `x.json: trailing comma at line 1 column 20`, `x.json: invalid type: string "1", expected i64 at line 1 column 31`. Свои сообщения docc («not a comments file of docc», «unsupported version», «comment 2 lacks id, doc or text», «no comments in it», «the docc-comments: line is damaged») остались прежними. То же касается `docc.comments.json`.
Почему: повторение текстов Go потребовало бы своего разборщика JSON примерно на 900 строк. Файлы замечаний пишет сама HTML-страница, поэтому испорченный файл — редкость, а номер строки помогает больше, чем точная формулировка.

### 2. Не принимаются ключи в другом регистре, повторы ключей и null вместо замечания
Было: Go читал `{"Docc":"comments","VERSION":1,...}` и `{"ID":"a",...}` так же, как ключи в нижнем регистре. При повторе ключа значения сливались: `"comments":[...],"comments":[...]` давал смесь обоих списков. `"comments":[null]` давал пустое замечание и ошибку «comment 1 lacks id, doc or text», а сам файл `null` — ошибку «not a comments file».
Стало: ключи сравниваются точно. Ключ `"ID"` пропускается как незнакомый, и тогда выходит «comment 1 lacks id, doc or text». При повторе ключа берётся одно значение, а не смесь. Для `[null]` и `null` выходит ошибка serde_json вместо этих сообщений. Как и в Go, сохранились: null вместо строки (`"quote":null`) пропускается, одиночная половинка эмодзи (`"\ud800"`) и неверный UTF-8 читаются как «�».
Почему: страница пишет файл через `JSON.stringify`, и такие ключи в нём не появляются. Слияние повторов — особенность Go, а не задуманное поведение.

### 3. Файл `[]` в docc.comments.json читается как пустой список
Было: Go отвечал `docc.comments.json: json: cannot unmarshal array into Go value of type comments.store`.
Стало: замечаний нет, ошибки нет.
Почему: так ведёт себя serde. Вреда нет: в файле нет ни одного замечания.
