/* The shop sample's compiled template (xerune_capi/examples/compiled):
 * constructors in place of xr_new / xr_app_new and xr_load_template. The
 * rest is xerune.h. */

#ifndef SHOP_H
#define SHOP_H

#include "xerune.h"

#ifdef __cplusplus
extern "C" {
#endif

XrUi *xr_new_shop(const XrFont *fonts, size_t count);
XrApp *xr_app_new_shop(const XrFont *fonts, size_t count);

#ifdef __cplusplus
}
#endif

#endif
