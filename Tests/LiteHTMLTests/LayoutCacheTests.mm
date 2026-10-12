#import <XCTest/XCTest.h>
#import "LHDocument.h"

#include <random>
#include <string>
#include <vector>

@interface LayoutCacheTests : XCTestCase
@end

@implementation LayoutCacheTests

static NSString *const Sheet = @"body{margin:4px;font:12px -apple-system}table{border-collapse:collapse}td{padding:1px 4px;border-bottom:1px solid #ddd}"
                                ".mid{vertical-align:middle}.bot{vertical-align:bottom}.top{vertical-align:top}.two{height:34px}"
                                ".ib{display:inline-block;padding:2px;border:1px solid #888}.fl{float:left;width:18px;height:10px;background:#c00}"
                                ".row{display:flex;gap:4px}.row>span{flex:1 1 auto}.card{padding:3px;border:1px solid #ccc;margin:2px 0}"
                                ".full{width:100%}.hot{color:#c00}";

static NSData *Image(LHDocument *document, CGFloat width)
{
    CGSize     size  = [document layoutWithMaxWidth:width preferredWidth:width];
    CGImageRef image = [document copyImageWithMaxWidth:width rect:CGRectMake(0, 0, width, MAX(1, ceil(size.height))) scale:2];
    if(!image)
        return [NSData data];
    NSData *data = (__bridge_transfer NSData *)CGDataProviderCopyData(CGImageGetDataProvider(image));
    CGImageRelease(image);
    return data;
}

static NSString *Page(const std::string &body)
{
    return [NSString stringWithFormat:@"<!DOCTYPE html><html data-lh-id=\"1\"><head data-lh-id=\"2\"><style data-lh-id=\"3\">%@</style></head>"
                                       "<body data-lh-id=\"4\">%@</body></html>",
                                       Sheet, @(body.c_str())];
}

- (void)expect:(LHDocument *)patched like:(LHDocument *)fresh width:(CGFloat)width label:(NSString *)label
{
    NSData *a = Image(patched, width), *b = Image(fresh, width);
    XCTAssertEqualObjects(patched.layoutSignature, fresh.layoutSignature, @"bounds %@", label);
    XCTAssertEqual(patched.size.height, fresh.size.height, @"height %@", label);
    XCTAssertEqualObjects(a, b, @"pixels %@", label);
}

static long Hits()
{
    return [LHDocument.layoutCacheStats[@"hits"] longValue];
}

- (void)testOneCellEditReusesTheOtherCells
{
    auto body = [](int frame) {
        std::string out = "<table data-lh-id=\"5\" class=\"full\">";
        int         id  = 10;
        for(int r = 0; r < 60; r++)
        {
            out += "<tr data-lh-id=\"" + std::to_string(id++) + "\"><td data-lh-id=\"" + std::to_string(id++) + "\">Row " + std::to_string(r) +
                   "</td><td data-lh-id=\"" + std::to_string(id++) + "\" class=\"" + (r == 7 && frame % 2 ? "hot" : "") + "\">" +
                   (r == 7 ? std::to_string(100 + frame * 7 % 800) + ".25" : std::to_string(r * 3) + ".50") + "</td></tr>";
        }
        return out + "</table>";
    };
    LHDocument *document = [[LHDocument alloc] initWithHTML:Page(body(0)) css:nil previous:nil];
    [document layoutWithMaxWidth:260 preferredWidth:260];
    for(int frame = 1; frame < 12; frame++)
    {
        NSString   *html   = Page(body(frame));
        long        before = Hits();
        LHDocument *patch  = [[LHDocument alloc] initWithHTML:html css:nil previous:document];
        LHDocument *fresh  = [[LHDocument alloc] initWithHTML:html css:nil];
        XCTAssertTrue(patch.reusedPrevious);
        [self expect:patch like:fresh width:260 label:[NSString stringWithFormat:@"frame %d", frame]];
        if(frame > 1)
            XCTAssertGreaterThan(Hits() - before, 100, @"frame %d", frame);
        document = patch;
    }
}

- (void)testRandomEditsAroundCachedBoxesMatchAFreshLayout
{
    const char  *budget   = getenv("TWIDGET_FUZZ_SECONDS");
    const double seconds  = budget ? atof(budget) : 0;
    NSDate      *deadline = [NSDate dateWithTimeIntervalSinceNow:seconds];
    long         hits     = Hits();
    for(unsigned seed = 1; seed <= 10 || [deadline timeIntervalSinceNow] > 0; seed++)
    {
        @autoreleasepool
        {
            std::mt19937             random(seed);
            const char              *words[]   = {"1.5", "12.50", "100.25", "4.0", "wide value 99", "alpha beta gamma", "x", "1041.09", "7"};
            const int                wordCount = sizeof(words) / sizeof(words[0]);
            const char              *aligns[]  = {"", "mid", "bot", "top"};
            std::vector<std::string> texts(48);
            std::vector<std::string> cellClass(48);
            for(size_t i = 0; i < texts.size(); i++)
            {
                texts[i]     = words[random() % wordCount];
                cellClass[i] = aligns[random() % 4];
            }
            const bool full = random() % 2;
            auto       body = [&]() {
                std::string out = std::string("<table data-lh-id=\"5\"") + (full ? " class=\"full\"" : "") + ">";
                int         id  = 10, t = 0;
                for(int r = 0; r < 6; r++)
                {
                    out += "<tr data-lh-id=\"" + std::to_string(id++) + "\">";
                    for(int c = 0; c < 4; c++, t++)
                    {
                        std::string cls = cellClass[t];
                        if(r == 2 && c == 0)
                            cls += " two";
                        out += "<td data-lh-id=\"" + std::to_string(id++) + "\" class=\"" + cls + "\">";
                        if(c == 1)
                            out += "<span data-lh-id=\"" + std::to_string(id++) + "\" class=\"ib\">" + texts[t] + "</span>";
                        else if(c == 2 && r % 2)
                            out += "<i data-lh-id=\"" + std::to_string(id++) + "\" class=\"fl\"></i>" + texts[t];
                        else
                            out += texts[t];
                        out += "</td>";
                    }
                    out += "</tr>";
                }
                out += "</table>";
                for(int k = 0; k < 6; k++, t++)
                    out += "<div data-lh-id=\"" + std::to_string(id++) + "\" class=\"row card\"><span data-lh-id=\"" + std::to_string(id++) + "\">Item " +
                           std::to_string(k) + "</span><span data-lh-id=\"" + std::to_string(id++) + "\" class=\"" + cellClass[t] + "\">" + texts[t] +
                           "</span></div>";
                out += "<p data-lh-id=\"" + std::to_string(id++) + "\">tail <span data-lh-id=\"" + std::to_string(id++) + "\" class=\"ib\">" + texts[t] +
                       "</span> end</p>";
                return out;
            };
            const CGFloat width    = 200 + random() % 160;
            LHDocument   *document = [[LHDocument alloc] initWithHTML:Page(body()) css:nil previous:nil];
            [document layoutWithMaxWidth:width preferredWidth:width];
            for(int step = 0; step < 24; step++)
            {
                const int edits = 1 + random() % 2;
                for(int e = 0; e < edits; e++)
                {
                    const size_t index = random() % 31;
                    if(random() % 4 == 0)
                        cellClass[index] = random() % 2 ? "hot" : aligns[random() % 4];
                    else
                        texts[index] = words[random() % wordCount];
                }
                NSString   *page  = Page(body());
                LHDocument *patch = [[LHDocument alloc] initWithHTML:page css:nil previous:document];
                LHDocument *fresh = [[LHDocument alloc] initWithHTML:page css:nil];
                [self expect:patch like:fresh width:width label:[NSString stringWithFormat:@"seed %u step %d", seed, step]];
                document = patch;
            }
        }
    }
    XCTAssertGreaterThan(Hits() - hits, 1000);
}

@end
