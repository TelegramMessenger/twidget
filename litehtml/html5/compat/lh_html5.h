#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct
{
    uint32_t start;
    uint32_t len;
} LhSpan;

typedef struct
{
    uint8_t  kind;
    uint8_t  ns;
    uint16_t tag;
    uint32_t parent;
    uint32_t first_child;
    uint32_t child_count;
    uint32_t first_attr;
    uint32_t attr_count;
    LhSpan   name;
    LhSpan   text;
    LhSpan   source;
} LhNode;

typedef struct
{
    uint32_t ns;
    LhSpan   name;
    LhSpan   value;
    LhSpan source;
} LhAttr;

typedef struct
{
    const LhNode*   nodes;
    uint32_t        node_count;
    uint32_t        root;
    const uint32_t* children;
    uint32_t        child_count;
    uint32_t        attr_count;
    const LhAttr*   attrs;
    const uint8_t*  strings;
    size_t          strings_len;
    uint8_t         quirks;
    uint8_t         truncated;
    uint8_t         positions;
} LhParse;

LhParse* lh_html5_parse(const uint8_t* input, size_t len, bool fragment);
void     lh_html5_free(LhParse* result);
uint32_t lh_html5_tag_count(void);

#ifdef __cplusplus
}
#endif
