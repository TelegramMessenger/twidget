#import "LHDocument.h"

#ifdef __cplusplus
#include "../core/lhx_painter.h"

@interface LHDocument (Painting)
- (CGSize)paintWithMaxWidth:(CGFloat)maxWidth clip:(CGRect)clip painter:(lhx::Painter &)painter;
@end
#endif
