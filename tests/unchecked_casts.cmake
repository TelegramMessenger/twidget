# cmake -DROOT=<twidget> -P unchecked_casts.cmake: fails when the renderer downcasts an element or a render item unchecked.
file(GLOB_RECURSE sources
    "${ROOT}/litehtml/src/*.cpp" "${ROOT}/litehtml/src/*.h" "${ROOT}/litehtml/include/*.h"
    "${ROOT}/core/src/*.cpp" "${ROOT}/core/src/*.h")
set(found "")
foreach(source IN LISTS sources)
    if(source MATCHES "/gumbo/")
        continue()
    endif()
    file(STRINGS "${source}" lines
        REGEX "static_pointer_cast|static_cast<[ ]*(const[ ]+)?(litehtml::)?(html_tag|el_[a-z_]+|render_[a-z_]+)[ ]*(const[ ]*)?[*&]")
    foreach(line IN LISTS lines)
        string(STRIP "${line}" line)
        string(APPEND found "  ${source}: ${line}\n")
    endforeach()
endforeach()
if(found)
    message(FATAL_ERROR "Unchecked downcast: use dynamic_cast or dynamic_pointer_cast and handle the miss.\n${found}")
endif()
