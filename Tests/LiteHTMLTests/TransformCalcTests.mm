#import <XCTest/XCTest.h>

#include "litehtml.h"
#include "css_parser.h"
#include "css_effects.h"

using namespace litehtml;

@interface TransformCalcTests : XCTestCase
@end

@implementation TransformCalcTests

- (void)testCarouselTransformsKeepPercentagesUntilLayout
{
    for(float width : {280.f, 400.f, 640.f})
    {
        for(int card = 0; card < 5; card++)
        {
            for(float drag : {-120.f, 0.f, 75.f})
            {
                const std::string css = "translateX(calc(11% - " + std::to_string(card) +
                                        " * (78% + 10px) + " + std::to_string(drag) + "px))";
                transform_vector functions;
                bool parsed = parse_transform(normalize(css, f_componentize | f_remove_whitespace), functions);
                XCTAssertTrue(parsed, @"%s", css.c_str());
                if(!parsed || functions.size() != 1)
                    continue;
                XCTAssertEqualWithAccuracy(functions[0].x.calc_percent(pixel_t(width)).value(),
                                           width * .11f - card * (width * .78f + 10) + drag, .001f);
            }
        }
    }
}

- (void)testEachTranslationUsesItsOwnAxis
{
    transform_vector functions;
    XCTAssertTrue(parse_transform(normalize("translate(calc(50% - 20px), calc((100% + 8px) / 2))",
                                            f_componentize | f_remove_whitespace), functions));
    if(functions.size() != 1)
        return;
    XCTAssertEqualWithAccuracy(functions[0].x.calc_percent(400_px).value(), 180, .001f);
    XCTAssertEqualWithAccuracy(functions[0].y.calc_percent(100_px).value(), 54, .001f);
    XCTAssertEqualWithAccuracy(functions[0].x.calc_percent(200_px).value(), 80, .001f);
    XCTAssertEqualWithAccuracy(functions[0].y.calc_percent(300_px).value(), 154, .001f);
}

@end
