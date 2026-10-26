#import <UIKit/UIKit.h>
static void rdd_present_frame(CGImageRef image);
#import <Foundation/Foundation.h>
#import <OpenGLES/EAGL.h>
#import <QuartzCore/CAEAGLLayer.h>
#import <OpenGLES/ES1/gl.h>
#import <OpenGLES/ES1/glext.h>
#include <string.h>
#include <stdio.h>
#include <pthread.h>
#include <mach/mach.h>

#include <signal.h>
#include <stdatomic.h>
#include <unistd.h>
static _Atomic(uint32_t) guestPC,guestLR;
void rdd_guest_context(uint32_t pc,uint32_t lr){atomic_store_explicit(&guestPC,pc,memory_order_relaxed);atomic_store_explicit(&guestLR,lr,memory_order_relaxed);}
static void rdd_crash_signal(int signalNumber){
 char line[]="FATAL native signal: guest_pc=00000000 guest_lr=00000000\n";
 uint32_t pc=atomic_load_explicit(&guestPC,memory_order_relaxed),lr=atomic_load_explicit(&guestLR,memory_order_relaxed);
 static const char hex[]="0123456789abcdef";
 for(int i=0;i<8;i++){line[30+i]=hex[(pc>>(28-4*i))&15];line[48+i]=hex[(lr>>(28-4*i))&15];}
 write(STDERR_FILENO,line,sizeof(line)-1);signal(signalNumber,SIG_DFL);raise(signalNumber);
}

static EAGLContext *context,*displayContext;
static CAEAGLLayer *displayLayer;static _Atomic(int) displayPixelWidth,displayPixelHeight;
static bool gpuPresentation;static int renderScale=1;static GLuint colorTexture,displayFramebuffer,displayRenderbuffer;static GLint drawableWidth,drawableHeight;
static GLuint framebuffer, renderbuffer,array_buffer,element_buffer;
int rdd_graphics_init(void) {
 gpuPresentation=getenv("RDD_GPU_PRESENT")&&strcmp(getenv("RDD_GPU_PRESENT"),"1")==0;renderScale=getenv("RDD_RENDER_SCALE")&&strcmp(getenv("RDD_RENDER_SCALE"),"2")==0?2:1;
 context=[[EAGLContext alloc] initWithAPI:kEAGLRenderingAPIOpenGLES1];
 if (!context || ![EAGLContext setCurrentContext:context]) return 1;
 glGenFramebuffersOES(1,&framebuffer);glBindFramebufferOES(GL_FRAMEBUFFER_OES,framebuffer);
 glGenRenderbuffersOES(1,&renderbuffer);glBindRenderbufferOES(GL_RENDERBUFFER_OES,renderbuffer);
 if(gpuPresentation){
  glGenTextures(1,&colorTexture);glBindTexture(GL_TEXTURE_2D,colorTexture);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MIN_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_MAG_FILTER,GL_LINEAR);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_S,GL_CLAMP_TO_EDGE);glTexParameteri(GL_TEXTURE_2D,GL_TEXTURE_WRAP_T,GL_CLAMP_TO_EDGE);
  glTexImage2D(GL_TEXTURE_2D,0,GL_RGBA,800*renderScale,480*renderScale,0,GL_RGBA,GL_UNSIGNED_BYTE,NULL);
  glFramebufferTexture2DOES(GL_FRAMEBUFFER_OES,GL_COLOR_ATTACHMENT0_OES,GL_TEXTURE_2D,colorTexture,0);glBindTexture(GL_TEXTURE_2D,0);
 }else{glRenderbufferStorageOES(GL_RENDERBUFFER_OES,GL_RGBA8_OES,800*renderScale,480*renderScale);glFramebufferRenderbufferOES(GL_FRAMEBUFFER_OES,GL_COLOR_ATTACHMENT0_OES,GL_RENDERBUFFER_OES,renderbuffer);}
 fprintf(stderr,"render configuration: GPU presentation=%d render_scale=%d source=800x480 backing=%dx%d\n",gpuPresentation,renderScale,800*renderScale,480*renderScale);
 GLenum status=glCheckFramebufferStatusOES(GL_FRAMEBUFFER_OES);
 printf("iOS graphics: %s / %s, framebuffer=0x%x, error=0x%x\n",glGetString(GL_RENDERER),glGetString(GL_VERSION),status,glGetError());
 return status==GL_FRAMEBUFFER_COMPLETE_OES?0:2;
}
static GLfloat f(uint32_t x){GLfloat v;memcpy(&v,&x,4);return v;}
int rdd_graphics_call(const char *name,uint32_t *a,uint8_t *memory,uint32_t *result) {
 #define IS(n) (!strcmp(name,#n))
 #define P(i) (a[i]?memory+(uint64_t)a[i]:NULL)
 #define V(i) (array_buffer?(const void*)(uintptr_t)a[i]:P(i))
 #define E(i) (element_buffer?(const void*)(uintptr_t)a[i]:P(i))
 *result=0;
 if(IS(glGenTextures))glGenTextures(a[0],(GLuint*)P(1));
 else if(IS(glDeleteTextures))glDeleteTextures(a[0],(GLuint*)P(1));
 else if(IS(glBindTexture))glBindTexture(a[0],a[1]);
 else if(IS(glTexImage2D))glTexImage2D(a[0],a[1],a[2],a[3],a[4],a[5],a[6],a[7],P(8));
 else if(IS(glTexSubImage2D))glTexSubImage2D(a[0],a[1],a[2],a[3],a[4],a[5],a[6],a[7],P(8));
 else if(IS(glTexParameteri))glTexParameteri(a[0],a[1],a[2]);
 else if(IS(glTexParameterx))glTexParameterx(a[0],a[1],a[2]);
 else if(IS(glTexParameterf))glTexParameterf(a[0],a[1],f(a[2]));
 else if(IS(glTexEnvx))glTexEnvx(a[0],a[1],a[2]);
 else if(IS(glTexEnvf))glTexEnvf(a[0],a[1],f(a[2]));
 else if(IS(glActiveTexture))glActiveTexture(a[0]);
 else if(IS(glClientActiveTexture))glClientActiveTexture(a[0]);
 else if(IS(glEnable))glEnable(a[0]);
 else if(IS(glDisable))glDisable(a[0]);
 else if(IS(glEnableClientState))glEnableClientState(a[0]);
 else if(IS(glDisableClientState))glDisableClientState(a[0]);
 else if(IS(glMatrixMode))glMatrixMode(a[0]);
 else if(IS(glLoadIdentity))glLoadIdentity();
 else if(IS(glLoadMatrixf))glLoadMatrixf((GLfloat*)P(0));
 else if(IS(glMultMatrixf))glMultMatrixf((GLfloat*)P(0));
 else if(IS(glPushMatrix))glPushMatrix();
 else if(IS(glPopMatrix))glPopMatrix();
 else if(IS(glTranslatef))glTranslatef(f(a[0]),f(a[1]),f(a[2]));
 else if(IS(glScalef))glScalef(f(a[0]),f(a[1]),f(a[2]));
 else if(IS(glRotatef))glRotatef(f(a[0]),f(a[1]),f(a[2]),f(a[3]));
 else if(IS(glOrthof))glOrthof(f(a[0]),f(a[1]),f(a[2]),f(a[3]),f(a[4]),f(a[5]));
 else if(IS(glViewport))glViewport((GLint)a[0]*renderScale,(GLint)a[1]*renderScale,a[2]*renderScale,a[3]*renderScale);
 else if(IS(glScissor))glScissor((GLint)a[0]*renderScale,(GLint)a[1]*renderScale,a[2]*renderScale,a[3]*renderScale);
 else if(IS(glColor4x))glColor4x(a[0],a[1],a[2],a[3]);
 else if(IS(glColor4f))glColor4f(f(a[0]),f(a[1]),f(a[2]),f(a[3]));
 else if(IS(glClearColor))glClearColor(f(a[0]),f(a[1]),f(a[2]),f(a[3]));
 else if(IS(glClear))glClear(a[0]);
 else if(IS(glBlendFunc))glBlendFunc(a[0],a[1]);
 else if(IS(glAlphaFunc))glAlphaFunc(a[0],f(a[1]));
 else if(IS(glVertexPointer))glVertexPointer(a[0],a[1],a[2],V(3));
 else if(IS(glColorPointer))glColorPointer(a[0],a[1],a[2],V(3));
 else if(IS(glTexCoordPointer))glTexCoordPointer(a[0],a[1],a[2],V(3));
 else if(IS(glDrawArrays))glDrawArrays(a[0],a[1],a[2]);
 else if(IS(glDrawElements))glDrawElements(a[0],a[1],a[2],E(3));
 else if(IS(glPixelStorei))glPixelStorei(a[0],a[1]);
 else if(IS(glGetIntegerv)){glGetIntegerv(a[0],(GLint*)P(1));if(a[0]==GL_VIEWPORT||a[0]==GL_SCISSOR_BOX){GLint *v=(GLint*)P(1);for(int i=0;i<4;i++)v[i]/=renderScale;}}
 else if(IS(glGetError))*result=glGetError();
 else if(IS(glFlush))glFlush();
 else if(IS(glFinish))glFinish();
 else if(IS(glBindBuffer)){glBindBuffer(a[0],a[1]);if(a[0]==GL_ARRAY_BUFFER)array_buffer=a[1];if(a[0]==GL_ELEMENT_ARRAY_BUFFER)element_buffer=a[1];}
 else if(IS(glGenBuffers))glGenBuffers(a[0],(GLuint*)P(1));
 else if(IS(glDeleteBuffers)){glDeleteBuffers(a[0],(GLuint*)P(1));glGetIntegerv(GL_ARRAY_BUFFER_BINDING,(GLint*)&array_buffer);glGetIntegerv(GL_ELEMENT_ARRAY_BUFFER_BINDING,(GLint*)&element_buffer);}
 else if(IS(glBufferData))glBufferData(a[0],(int32_t)a[1],P(2),a[3]);
 else if(IS(glLoadMatrixx))glLoadMatrixx((GLfixed*)P(0));
 else if(IS(glMultMatrixx))glMultMatrixx((GLfixed*)P(0));
 else if(IS(glLineWidthx))glLineWidthx(a[0]*renderScale);
 else if(IS(glPointSizex))glPointSizex(a[0]*renderScale);
 else if(IS(glGetFloatv))glGetFloatv(a[0],(GLfloat*)P(1));
 else if(IS(glGetFixedv))glGetFixedv(a[0],(GLfixed*)P(1));
 else if(IS(glClearColorx))glClearColorx(a[0],a[1],a[2],a[3]);
 else if(IS(glScalex))glScalex(a[0],a[1],a[2]);
 else if(IS(glRotatex))glRotatex(a[0],a[1],a[2],a[3]);
 else if(IS(glTranslatex))glTranslatex(a[0],a[1],a[2]);
 else if(IS(glOrthox))glOrthox(a[0],a[1],a[2],a[3],a[4],a[5]);
 else if(IS(glFrustumx))glFrustumx(a[0],a[1],a[2],a[3],a[4],a[5]);
 else if(IS(glLightxv))glLightxv(a[0],a[1],(GLfixed*)P(2));
 else if(IS(glDepthMask))glDepthMask(a[0]);
 else if(IS(glStencilMask))glStencilMask(a[0]);
 else if(IS(glColorMask))glColorMask(a[0],a[1],a[2],a[3]);
 else if(IS(glShadeModel))glShadeModel(a[0]);
 else if(IS(glNormalPointer))glNormalPointer(a[0],a[1],V(2));
 else if(IS(glIsTexture))*result=glIsTexture(a[0]);
 else if(IS(glIsEnabled))*result=glIsEnabled(a[0]);
 else if(IS(glGetTexParameteriv))glGetTexParameteriv(a[0],a[1],(GLint*)P(2));
 else if(IS(glCompressedTexImage2D))glCompressedTexImage2D(a[0],a[1],a[2],a[3],a[4],a[5],a[6],P(7));
 else if(IS(glTexEnvxv))glTexEnvxv(a[0],a[1],(GLfixed*)P(2));
 else if(IS(glCopyTexSubImage2D))glCopyTexSubImage2D(a[0],a[1],a[2],a[3],a[4],a[5],a[6],a[7]);
 else if(IS(glCopyTexImage2D))glCopyTexImage2D(a[0],a[1],a[2],a[3],a[4],a[5],a[6],a[7]);
 else return 1;
 return 0;
}
#include <sys/mount.h>
int rdd_filesystem_info(const char *path,uint64_t *values) {
 struct statfs s;if(statfs(path,&s))return -1;
 values[0]=s.f_bsize;values[1]=s.f_blocks;values[2]=s.f_bfree;values[3]=s.f_bavail;values[4]=s.f_files;values[5]=s.f_ffree;return 0;
}
int rdd_locale(int language,char *out,size_t capacity){@autoreleasepool{
 NSString *value=[[NSLocale currentLocale] objectForKey:language?NSLocaleLanguageCode:NSLocaleCountryCode];
 if(!value)return -1;const char *text=[value UTF8String];size_t n=strlen(text);if(n+1>capacity)return -1;memcpy(out,text,n+1);return (int)n;
}}

#include <sys/sysctl.h>
int rdd_device_model(char *out,size_t capacity){size_t length=capacity;if(sysctlbyname("hw.machine",out,&length,NULL,0)!=0||length==0||out[length-1]!=0)return -1;return (int)(length-1);}

#import <CoreText/CoreText.h>

// Only the engine worker calls these; bound the cache and preserve original font/size.
static CTFontRef rdd_copy_font(const char *path,double size){
 static NSMutableDictionary *cache; if(!cache)cache=[NSMutableDictionary new];
 NSString *key=[NSString stringWithFormat:@"%s:%.17g",path,size];
 id cached=cache[key];if(cached)return (CTFontRef)CFRetain((__bridge CFTypeRef)cached);
 CGDataProviderRef provider=CGDataProviderCreateWithURL((__bridge CFURLRef)[NSURL fileURLWithPath:[NSString stringWithUTF8String:path]]);if(!provider)return NULL;
 CGFontRef graphics=CGFontCreateWithDataProvider(provider);CGDataProviderRelease(provider);if(!graphics)return NULL;
 CTFontRef font=CTFontCreateWithGraphicsFont(graphics,size,NULL,NULL);CGFontRelease(graphics);
 if(font){if(cache.count>=32)[cache removeAllObjects];cache[key]=(__bridge id)font;}return font;
}

double rdd_text_width(const char *fontpath,const uint16_t *text,size_t length,double size){@autoreleasepool{
 CTFontRef font=rdd_copy_font(fontpath,size);if(!font)return -1;
 NSString *string=[[NSString alloc] initWithCharacters:text length:length];
 NSAttributedString *attributed=[[NSAttributedString alloc] initWithString:string attributes:@{(__bridge NSString*)kCTFontAttributeName:(__bridge id)font}];
 CTLineRef line=CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)attributed);double width=CTLineGetTypographicBounds(line,NULL,NULL,NULL);CFRelease(line);CFRelease(font);return width;
}}

int rdd_text_bitmap(const char *fontpath,const uint16_t *text,size_t length,double size,uint32_t width,uint32_t height,int underline,uint32_t *pixels){@autoreleasepool{
 if(!width||!height)return 0;
 CTFontRef font=rdd_copy_font(fontpath,size);if(!font)return -1;
 NSString *string=[[NSString alloc] initWithCharacters:text length:length];
 NSDictionary *attrs=@{(__bridge NSString*)kCTFontAttributeName:(__bridge id)font,(__bridge NSString*)kCTForegroundColorFromContextAttributeName:@YES};
 NSAttributedString *attributed=[[NSAttributedString alloc] initWithString:string attributes:attrs];
 CTLineRef line=CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)attributed);
 uint8_t *rgba=calloc(width*height,4);if(!rgba){CFRelease(line);CFRelease(font);return -1;}CGColorSpaceRef space=CGColorSpaceCreateDeviceRGB();
 CGContextRef ctx=CGBitmapContextCreate(rgba,width,height,8,width*4,space,kCGBitmapByteOrder32Big|kCGImageAlphaPremultipliedLast);CGColorSpaceRelease(space);
 if(!ctx){free(rgba);CFRelease(line);CFRelease(font);return -1;}
 CGContextSetRGBFillColor(ctx,1,1,1,1);CGContextSetTextPosition(ctx,0,height-size);CTLineDraw(line,ctx);
 if(underline){CGContextSetRGBStrokeColor(ctx,1,1,1,1);CGContextSetLineWidth(ctx,CTFontGetUnderlineThickness(font));CGFloat y=height-size+CTFontGetUnderlinePosition(font);CGContextMoveToPoint(ctx,0,y);CGContextAddLineToPoint(ctx,width,y);CGContextStrokePath(ctx);}
 for(uint32_t y=0;y<height;y++)for(uint32_t x=0;x<width;x++){uint8_t alpha=rgba[(y*width+x)*4+3];pixels[y*width+x]=alpha?((uint32_t)alpha<<24)|0x00ffffff:0;}
 CGContextRelease(ctx);free(rgba);CFRelease(line);CFRelease(font);return 0;
}}

#include <time.h>
int rdd_localtime(int64_t seconds,int32_t *fields,int64_t *offset,char *zone,size_t capacity){
 time_t value=(time_t)seconds;struct tm tm;if(!localtime_r(&value,&tm))return -1;
 int32_t parts[]={tm.tm_sec,tm.tm_min,tm.tm_hour,tm.tm_mday,tm.tm_mon,tm.tm_year,tm.tm_wday,tm.tm_yday,tm.tm_isdst};memcpy(fields,parts,sizeof(parts));*offset=tm.tm_gmtoff;
 const char *name=tm.tm_zone?tm.tm_zone:"";size_t n=strlen(name);if(n+1>capacity)return -1;memcpy(zone,name,n+1);return 0;
}

#import <ImageIO/ImageIO.h>
static uint8_t frameRaw[1600*960*4],frameTop[1600*960*4];

static int rdd_gpu_present(void){
 int expectedWidth=atomic_load(&displayPixelWidth),expectedHeight=atomic_load(&displayPixelHeight);if(!displayLayer||!expectedWidth||!expectedHeight)return 0;
 // Separate shared context: the presentation pass cannot mutate game GL state.
 glFinish();
 if(!displayContext){displayContext=[[EAGLContext alloc] initWithAPI:kEAGLRenderingAPIOpenGLES1 sharegroup:context.sharegroup];if(!displayContext)return 4;}
 if(![EAGLContext setCurrentContext:displayContext])return 4;
 int result=0;
 if(!displayFramebuffer){glGenFramebuffersOES(1,&displayFramebuffer);glGenRenderbuffersOES(1,&displayRenderbuffer);}
 glBindFramebufferOES(GL_FRAMEBUFFER_OES,displayFramebuffer);glBindRenderbufferOES(GL_RENDERBUFFER_OES,displayRenderbuffer);
 if(drawableWidth!=expectedWidth||drawableHeight!=expectedHeight){
  if(![displayContext renderbufferStorage:GL_RENDERBUFFER_OES fromDrawable:displayLayer]){result=5;goto restore;}
  glGetRenderbufferParameterivOES(GL_RENDERBUFFER_OES,GL_RENDERBUFFER_WIDTH_OES,&drawableWidth);glGetRenderbufferParameterivOES(GL_RENDERBUFFER_OES,GL_RENDERBUFFER_HEIGHT_OES,&drawableHeight);
  glFramebufferRenderbufferOES(GL_FRAMEBUFFER_OES,GL_COLOR_ATTACHMENT0_OES,GL_RENDERBUFFER_OES,displayRenderbuffer);
  if(glCheckFramebufferStatusOES(GL_FRAMEBUFFER_OES)!=GL_FRAMEBUFFER_COMPLETE_OES){result=6;goto restore;}
  fprintf(stderr,"GPU drawable: %dx%d\n",drawableWidth,drawableHeight);
 }
 glDisable(GL_SCISSOR_TEST);glDisable(GL_BLEND);glDisable(GL_DEPTH_TEST);glClearColor(0,0,0,1);glClear(GL_COLOR_BUFFER_BIT);
 {GLfloat scale=MIN(drawableWidth/800.f,drawableHeight/480.f);GLint width=lroundf(800*scale),height=lroundf(480*scale);glViewport((drawableWidth-width)/2,(drawableHeight-height)/2,width,height);}
 glMatrixMode(GL_PROJECTION);glLoadIdentity();glOrthof(0,1,0,1,-1,1);glMatrixMode(GL_MODELVIEW);glLoadIdentity();glMatrixMode(GL_TEXTURE);glLoadIdentity();
 glEnable(GL_TEXTURE_2D);glBindTexture(GL_TEXTURE_2D,colorTexture);glColor4f(1,1,1,1);glTexEnvf(GL_TEXTURE_ENV,GL_TEXTURE_ENV_MODE,GL_MODULATE);
 {const GLfloat positions[]={0,0,1,0,0,1,1,1},uv[]={0,0,1,0,0,1,1,1};glEnableClientState(GL_VERTEX_ARRAY);glEnableClientState(GL_TEXTURE_COORD_ARRAY);glVertexPointer(2,GL_FLOAT,0,positions);glTexCoordPointer(2,GL_FLOAT,0,uv);glDrawArrays(GL_TRIANGLE_STRIP,0,4);}
 if(![displayContext presentRenderbuffer:GL_RENDERBUFFER_OES]){result=7;goto restore;}
 {static double last;static unsigned count;double now=CFAbsoluteTimeGetCurrent();if(!last)last=now;count++;if(now-last>=5){struct task_vm_info vm={0};mach_msg_type_number_t n=TASK_VM_INFO_COUNT;task_info(mach_task_self(),TASK_VM_INFO,(task_info_t)&vm,&n);fprintf(stderr,"GPU presentation: %.2f fps footprint_mb=%.1f rss_mb=%.1f\n",count/(now-last),vm.phys_footprint/1048576.,vm.resident_size/1048576.);last=now;count=0;}}
restore:
 if(![EAGLContext setCurrentContext:context])return 4;
 if(result)fprintf(stderr,"GPU presentation error=%d\n",result);return result;
}

int rdd_graphics_present(void) {
 if(gpuPresentation)return rdd_gpu_present();
 const size_t width=800*renderScale,height=480*renderScale,row=width*4;
 uint8_t *raw=frameRaw,*top=frameTop;
 glReadPixels(0,0,width,height,GL_RGBA,GL_UNSIGNED_BYTE,raw);
 GLenum error=glGetError();if(error){return 1;}
 for(size_t y=0;y<height;y++)memcpy(top+y*row,raw+(height-1-y)*row,row);
 CGColorSpaceRef colors=CGColorSpaceCreateDeviceRGB();
 CGContextRef bitmap=CGBitmapContextCreate(top,width,height,8,row,colors,kCGBitmapByteOrder32Big|kCGImageAlphaPremultipliedLast);
 if(!bitmap){CGColorSpaceRelease(colors);return 3;}
 CGImageRef image=CGBitmapContextCreateImage(bitmap);if(!image){CGContextRelease(bitmap);CGColorSpaceRelease(colors);return 3;}rdd_present_frame(image);
 CGImageRelease(image);CGContextRelease(bitmap);CGColorSpaceRelease(colors);return 0;
}

int rdd_graphics_snapshot(const char *path) {
 const size_t width=800*renderScale,height=480*renderScale,row=width*4;
 uint8_t *raw=frameRaw,*top=frameTop;
 glFinish();glReadPixels(0,0,width,height,GL_RGBA,GL_UNSIGNED_BYTE,raw);
 GLenum error=glGetError();if(error){fprintf(stderr,"capture GL error %x\n",error);return 1;}
 for(size_t y=0;y<height;y++)memcpy(top+y*row,raw+(height-1-y)*row,row);
 CGColorSpaceRef colors=CGColorSpaceCreateDeviceRGB();
 CGContextRef bitmap=CGBitmapContextCreate(top,width,height,8,row,colors,kCGBitmapByteOrder32Big|kCGImageAlphaPremultipliedLast);
 if(!bitmap){CGColorSpaceRelease(colors);return 3;}
 CGImageRef image=CGBitmapContextCreateImage(bitmap);if(!image){CGContextRelease(bitmap);CGColorSpaceRelease(colors);return 3;}
 CFURLRef url=CFURLCreateFromFileSystemRepresentation(NULL,(const UInt8*)path,strlen(path),false);
 CGImageDestinationRef destination=CGImageDestinationCreateWithURL(url,CFSTR("public.png"),1,NULL);
 if(destination)CGImageDestinationAddImage(destination,image,NULL);
 bool saved=destination&&CGImageDestinationFinalize(destination);
 if(destination)CFRelease(destination);CFRelease(url);CGImageRelease(image);CGContextRelease(bitmap);CGColorSpaceRelease(colors);return saved?0:2;
}

#include <CommonCrypto/CommonDigest.h>
int rdd_md5(const uint8_t *bytes,uint32_t length,uint8_t *digest){return CC_MD5(bytes,length,digest)?0:1;}

int64_t rdd_raw_timezone_offset_ms(void) {@autoreleasepool{ NSTimeZone *zone=[NSTimeZone localTimeZone]; NSDate *now=[NSDate date];return ((int64_t)[zone secondsFromGMTForDate:now]-(int64_t)[zone daylightSavingTimeOffsetForDate:now])*1000; }}

static UIImageView *gameView;
static pthread_mutex_t frameMutex=PTHREAD_MUTEX_INITIALIZER;
static UIImage *pendingFrame;static bool frameQueued;
static void rdd_present_frame(CGImageRef frame){@autoreleasepool{
 if(!gameView)return;UIImage *image=[UIImage imageWithCGImage:frame];
 pthread_mutex_lock(&frameMutex);pendingFrame=image;bool enqueue=!frameQueued;frameQueued=true;pthread_mutex_unlock(&frameMutex);
 if(enqueue)dispatch_async(dispatch_get_main_queue(),^{
  pthread_mutex_lock(&frameMutex);UIImage *next=pendingFrame;pendingFrame=nil;frameQueued=false;pthread_mutex_unlock(&frameMutex);gameView.image=next;
  static double last;static unsigned count;double now=CFAbsoluteTimeGetCurrent();if(!last)last=now;count++;if(now-last>=5){struct task_vm_info vm={0};mach_msg_type_number_t vmcount=TASK_VM_INFO_COUNT;kern_return_t status=task_info(mach_task_self(),TASK_VM_INFO,(task_info_t)&vm,&vmcount);fprintf(stderr,"presentation: %.2f fps over %.2fs, footprint_mb=%.1f rss_mb=%.1f memory_status=%d\n",count/(now-last),now-last,vm.phys_footprint/1048576.,vm.resident_size/1048576.,status);count=0;last=now;}
 });
}}
void rdd_vibrate(int64_t milliseconds){if(milliseconds<=0)return;dispatch_async(dispatch_get_main_queue(),^{UIImpactFeedbackGenerator *feedback=[[UIImpactFeedbackGenerator alloc] initWithStyle:UIImpactFeedbackStyleLight];[feedback impactOccurred];});}

extern void rdd_run_game(void);
extern int rdd_save_apk_icon(const char *apk,const char *out);
#include <pthread.h>
static pthread_mutex_t touchMutex=PTHREAD_MUTEX_INITIALIZER;
static int32_t touchQueue[256][4];static unsigned touchRead,touchWrite;static bool touching;
static void queueTouch(int32_t type,CGPoint point,int32_t count){CGFloat scale=MIN(gameView.bounds.size.width/800.,gameView.bounds.size.height/480.);CGFloat left=(gameView.bounds.size.width-800*scale)/2,top=(gameView.bounds.size.height-480*scale)/2;int32_t x=(point.x-left)/scale,y=(point.y-top)/scale;if(type==(int32_t)0x80000001&&(x<0||x>=800||y<0||y>=480))return;pthread_mutex_lock(&touchMutex);if(touchWrite-touchRead>=256){fprintf(stderr,"touch queue overflow\n");abort();}int32_t *e=touchQueue[touchWrite++%256];e[0]=type;e[1]=MAX(0,MIN(799,x));e[2]=MAX(0,MIN(479,y));e[3]=count;pthread_mutex_unlock(&touchMutex);}
int rdd_next_touch(int32_t *values){pthread_mutex_lock(&touchMutex);bool available=touchRead!=touchWrite;if(available)memcpy(values,touchQueue[touchRead++%256],sizeof(touchQueue[0]));pthread_mutex_unlock(&touchMutex);return available?1:0;}
@interface RDDGameView:UIImageView
@end
// Test hook (RDD_AUTOTEST_TOUCHES=1): runs Documents/autotest-taps.txt, then deletes it.
// Lines, in 800x480 game coordinates: "tap X Y", "drag X1 Y1 X2 Y2", "wait MS".
static void queueGameTouch(int32_t type,int32_t x,int32_t y){pthread_mutex_lock(&touchMutex);if(touchWrite-touchRead<256){int32_t *e=touchQueue[touchWrite++%256];e[0]=type;e[1]=x;e[2]=y;e[3]=1;}pthread_mutex_unlock(&touchMutex);}
static void RDDStartTouchScript(void){if(!getenv("RDD_AUTOTEST_TOUCHES"))return;
 dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY,0),^{NSString *path=[NSSearchPathForDirectoriesInDomains(NSDocumentDirectory,NSUserDomainMask,YES).firstObject stringByAppendingPathComponent:@"autotest-taps.txt"];
  for(;;){usleep(200000);NSString *script=[NSString stringWithContentsOfFile:path encoding:NSUTF8StringEncoding error:nil];if(!script)continue;[NSFileManager.defaultManager removeItemAtPath:path error:nil];
   for(NSString *line in [script componentsSeparatedByString:@"\n"]){NSArray<NSString*> *f=[line componentsSeparatedByCharactersInSet:NSCharacterSet.whitespaceCharacterSet];
    if([f[0] isEqualToString:@"wait"]&&f.count>1)usleep(f[1].intValue*1000);
    else if([f[0] isEqualToString:@"tap"]&&f.count>2){queueGameTouch(0x80000001,f[1].intValue,f[2].intValue);usleep(80000);queueGameTouch(0x80000003,f[1].intValue,f[2].intValue);usleep(150000);}
    else if([f[0] isEqualToString:@"drag"]&&f.count>4){int x1=f[1].intValue,y1=f[2].intValue,x2=f[3].intValue,y2=f[4].intValue;queueGameTouch(0x80000001,x1,y1);for(int k=1;k<=10;k++){usleep(30000);queueGameTouch(0x80000002,x1+(x2-x1)*k/10,y1+(y2-y1)*k/10);}queueGameTouch(0x80000003,x2,y2);usleep(150000);}
    fprintf(stderr,"autotest touch: %s\n",line.UTF8String);}}});}
@implementation RDDGameView
- (void)layoutSubviews{[super layoutSubviews];if(displayLayer){displayLayer.frame=self.bounds;atomic_store(&displayPixelWidth,lroundf(self.bounds.size.width*displayLayer.contentsScale));atomic_store(&displayPixelHeight,lroundf(self.bounds.size.height*displayLayer.contentsScale));}}
- (void)touchesBegan:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event{if(touching)return;touching=true;queueTouch(0x80000001,[touches.anyObject locationInView:self],1);}
- (void)touchesMoved:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event{if(touching)queueTouch(0x80000002,[touches.anyObject locationInView:self],1);}
- (void)touchesEnded:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event{if(touching){queueTouch(0x80000003,[touches.anyObject locationInView:self],1);touching=false;}}
- (void)touchesCancelled:(NSSet<UITouch*>*)touches withEvent:(UIEvent*)event{[self touchesEnded:touches withEvent:event];}
@end
@interface RDDGameController:UIViewController
@end
@implementation RDDGameController
- (void)viewDidLoad{[super viewDidLoad];self.view.backgroundColor=UIColor.blackColor;gameView=[[RDDGameView alloc] initWithFrame:self.view.bounds];gameView.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;gameView.contentMode=UIViewContentModeScaleAspectFit;gameView.userInteractionEnabled=YES;[self.view addSubview:gameView];if(getenv("RDD_GPU_PRESENT")&&strcmp(getenv("RDD_GPU_PRESENT"),"1")==0){displayLayer=[CAEAGLLayer layer];displayLayer.opaque=YES;displayLayer.contentsScale=UIScreen.mainScreen.scale;displayLayer.drawableProperties=@{kEAGLDrawablePropertyRetainedBacking:@NO,kEAGLDrawablePropertyColorFormat:kEAGLColorFormatRGBA8};[gameView.layer addSublayer:displayLayer];[gameView setNeedsLayout];[gameView layoutIfNeeded];}}
- (BOOL)prefersStatusBarHidden{return YES;}
- (UIInterfaceOrientationMask)supportedInterfaceOrientations{return UIInterfaceOrientationMaskLandscape;}
@end
// ---- First-launch APK import -------------------------------------------------
// The app ships without any game content. The player supplies their own copy of
// the Android APK once; it is checked against the supported build and kept in
// Documents/runtime-data/original.apk, where the engine reads it on every launch.
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
static NSString *const RDDSupportedAPKSHA256=@"50b1cd98240e61f0834f5bc40e765d7075ac4835a76d271a4edda350efdf9221";
static BOOL gameStarted;
static NSString *RDDInstalledAPKPath(void){NSString *documents=NSSearchPathForDirectoriesInDomains(NSDocumentDirectory,NSUserDomainMask,YES).firstObject;return [documents stringByAppendingPathComponent:@"runtime-data/original.apk"];}
static NSString *RDDSHA256OfFile(NSString *path){
 NSInputStream *in=[NSInputStream inputStreamWithFileAtPath:path];if(!in)return nil;[in open];
 CC_SHA256_CTX ctx;CC_SHA256_Init(&ctx);uint8_t *buffer=malloc(1<<20);NSInteger n;
 while((n=[in read:buffer maxLength:1<<20])>0)CC_SHA256_Update(&ctx,buffer,(CC_LONG)n);
 free(buffer);[in close];if(n<0)return nil;unsigned char digest[CC_SHA256_DIGEST_LENGTH];CC_SHA256_Final(digest,&ctx);
 NSMutableString *hex=[NSMutableString string];for(int i=0;i<CC_SHA256_DIGEST_LENGTH;i++)[hex appendFormat:@"%02x",digest[i]];return hex;
}
/// Documents/runtime-data/icon.png: the game's launcher icon, copied out of the player's APK.
/// iOS only lets an app switch to icons signed into its bundle, so `build.py device` reads this
/// file off the phone and builds it into the next (weekly) install instead.
static NSString *RDDAPKIconPath(void){return [RDDInstalledAPKPath().stringByDeletingLastPathComponent stringByAppendingPathComponent:@"icon.png"];}
static void RDDSaveAPKIcon(void){if(![NSFileManager.defaultManager fileExistsAtPath:RDDAPKIconPath()])rdd_save_apk_icon(RDDInstalledAPKPath().fileSystemRepresentation,RDDAPKIconPath().fileSystemRepresentation);}
static BOOL RDDInstalledAPKIsValid(void){
 NSString *path=RDDInstalledAPKPath();if(![NSFileManager.defaultManager fileExistsAtPath:path])return NO;
 NSString *marker=[path stringByAppendingString:@".verified"];NSDictionary *a=[NSFileManager.defaultManager attributesOfItemAtPath:path error:nil];
 NSString *stamp=[NSString stringWithFormat:@"%@ %@ %@",RDDSupportedAPKSHA256,a[NSFileSize],@([a[NSFileModificationDate] timeIntervalSince1970])];
 if([[NSString stringWithContentsOfFile:marker encoding:NSUTF8StringEncoding error:nil] isEqualToString:stamp])return YES;
 if(![RDDSHA256OfFile(path) isEqualToString:RDDSupportedAPKSHA256])return NO;
 [stamp writeToFile:marker atomically:YES encoding:NSUTF8StringEncoding error:nil];return YES;
}
/// Copies an APK from `url` into place after checking it. Returns nil on success or a message for the player.
static NSString *RDDImportAPK(NSURL *url){
 BOOL scoped=[url startAccessingSecurityScopedResource];
 NSString *hash=RDDSHA256OfFile(url.path);
 NSString *result=nil;
 if(!hash)result=@"That file could not be read. Try saving it to On My iPhone in the Files app first.";
 else if(![hash isEqualToString:RDDSupportedAPKSHA256])result=[NSString stringWithFormat:@"This isn't the supported APK.\n\nNeeded: 9 Innings: Pro Baseball 2016, version 6.0.7 (58 MB).\nExpected SHA-256: %@…\nThis file: %@…",[RDDSupportedAPKSHA256 substringToIndex:16],[hash substringToIndex:16]];
 else{
  NSString *path=RDDInstalledAPKPath();NSFileManager *fm=NSFileManager.defaultManager;NSError *e=nil;
  [fm createDirectoryAtPath:path.stringByDeletingLastPathComponent withIntermediateDirectories:YES attributes:nil error:nil];
  NSString *temp=[path stringByAppendingString:@".importing"];[fm removeItemAtPath:temp error:nil];
  if(![fm copyItemAtPath:url.path toPath:temp error:&e])result=[NSString stringWithFormat:@"Copy failed: %@",e.localizedDescription];
  else{[fm removeItemAtPath:path error:nil];[fm moveItemAtPath:temp toPath:path error:nil];[fm removeItemAtPath:[path stringByAppendingString:@".verified"] error:nil];[fm removeItemAtPath:RDDAPKIconPath() error:nil];if(!RDDInstalledAPKIsValid())result=@"The copied file failed verification. Please try again.";else RDDSaveAPKIcon();}
 }
 if(scoped)[url stopAccessingSecurityScopedResource];
 fprintf(stderr,"apk import from %s: %s\n",url.lastPathComponent.UTF8String,result?result.UTF8String:"ok");
 return result;
}
/// Picks up an APK placed in the app's Documents folder (Finder file sharing, Files app)
/// or bundled into a personal developer build.
static void RDDAdoptBundledOrSharedAPK(void){
 if(RDDInstalledAPKIsValid())return;NSFileManager *fm=NSFileManager.defaultManager;
 NSString *documents=NSSearchPathForDirectoriesInDomains(NSDocumentDirectory,NSUserDomainMask,YES).firstObject;
 NSMutableArray<NSString*> *candidates=[NSMutableArray array];
 NSString *bundled=[NSBundle.mainBundle pathForResource:@"original" ofType:@"apk"];if(bundled)[candidates addObject:bundled];
 for(NSString *name in [fm contentsOfDirectoryAtPath:documents error:nil])if([name.pathExtension.lowercaseString isEqualToString:@"apk"])[candidates addObject:[documents stringByAppendingPathComponent:name]];
 // A shared copy in Documents is redundant once imported; drop it to save 60 MB.
 for(NSString *c in candidates)if(!RDDImportAPK([NSURL fileURLWithPath:c])){if(![c isEqualToString:bundled])[fm removeItemAtPath:c error:nil];return;}
}
// ---- In-app game download -----------------------------------------------------
// Opens a public mirror page (supported-apks.json) in a real web view. When the page
// starts an APK download, the file is captured, verified by SHA-256 and imported.
#import <WebKit/WebKit.h>
static NSArray<NSDictionary*> *RDDDownloadSources(void){
 NSData *json=[NSData dataWithContentsOfFile:[NSBundle.mainBundle pathForResource:@"supported-apks" ofType:@"json"]];
 NSDictionary *manifest=json?[NSJSONSerialization JSONObjectWithData:json options:0 error:nil]:nil;
 NSMutableArray *sources=[NSMutableArray array];
 for(NSDictionary *apk in manifest[@"apks"])if([apk[@"sha256"] isEqualToString:RDDSupportedAPKSHA256])[sources addObjectsFromArray:apk[@"sources"]?:@[]];
 return sources;
}
@interface RDDDownloadController:UIViewController<WKNavigationDelegate,WKDownloadDelegate,WKUIDelegate>
@property(copy,nonatomic) void(^completion)(NSString *error);
@property(copy,nonatomic) NSURL *page;
@end
// Content-blocker rules (the format Safari content blockers use; url-filter has no | alternation): drop ad/tracker requests and hide ad slots,
// so the mirror page is just the download button if it ever has to be shown.
static NSString *const RDDAdBlockRules=@"["
 "{\"trigger\":{\"url-filter\":\".*\",\"if-domain\":[\"*doubleclick.net\",\"*googlesyndication.com\",\"*googletagservices.com\",\"*googletagmanager.com\",\"*google-analytics.com\",\"*adservice.google.com\",\"*amazon-adsystem.com\",\"*taboola.com\",\"*outbrain.com\",\"*criteo.com\",\"*criteo.net\",\"*adnxs.com\",\"*pubmatic.com\",\"*rubiconproject.com\",\"*openx.net\",\"*casalemedia.com\",\"*moatads.com\",\"*quantserve.com\",\"*scorecardresearch.com\",\"*media.net\",\"*sharethrough.com\",\"*33across.com\",\"*sovrn.com\",\"*lijit.com\",\"*indexww.com\",\"*adsafeprotected.com\",\"*doubleverify.com\",\"*facebook.net\",\"*hotjar.com\",\"*onesignal.com\"]},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"doubleclick\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"googlesyndication\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"adservice\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"amazon-adsystem\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"taboola\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"outbrain\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"pagead\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\"prebid\"},\"action\":{\"type\":\"block\"}},"
 "{\"trigger\":{\"url-filter\":\".*\"},\"action\":{\"type\":\"css-display-none\",\"selector\":\"ins.adsbygoogle,[id^=div-gpt-ad],[id^=google_ads],[class*=ad-container],[class*=ad-slot],[class*=google-ad],[class*=ai-viewport],.ezoic-ad,[id^=ezoic],iframe[src*=ads],#ad-banner,.adsbox,.sticky-ad,[class*=sticky-footer-ad],[class*=OUTBRAIN],[id*=taboola]\"}}"
 "]";
// Presses the mirror's own download buttons: the release page's "Download APK" button, then the
// "click here" link on the page that hands out the file.
static NSString *const RDDPressDownloadJS=@"(function(){var a=document.querySelector('#download-link')||document.querySelector('a.downloadButton')||document.querySelector('a[href*=\"download.php\"]')||document.querySelector('a[href*=\"/download/?key=\"]');if(a){a.removeAttribute('target');a.click();return a.href}return 'none: '+document.title})()";
@implementation RDDDownloadController{WKWebView *web;UIProgressView *bar;UILabel *banner;NSURL *destination;WKDownload *active;UIView *cover;UILabel *coverText;UIProgressView *coverBar;BOOL revealed;}
- (void)viewDidLoad{[super viewDidLoad];self.view.backgroundColor=UIColor.systemBackgroundColor;
 WKWebViewConfiguration *config=[WKWebViewConfiguration new];config.websiteDataStore=WKWebsiteDataStore.nonPersistentDataStore;
 web=[[WKWebView alloc] initWithFrame:CGRectZero configuration:config];web.navigationDelegate=self;web.UIDelegate=self;web.allowsBackForwardNavigationGestures=YES;
 web.customUserAgent=@"Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
 banner=[UILabel new];banner.text=@"Tap the site's Download button for the APK — the game installs itself when the download finishes.";banner.numberOfLines=0;banner.font=[UIFont systemFontOfSize:13 weight:UIFontWeightSemibold];banner.textAlignment=NSTextAlignmentCenter;banner.backgroundColor=[UIColor colorWithRed:0.05 green:0.09 blue:0.16 alpha:1];banner.textColor=UIColor.whiteColor;
 UIButton *close=[UIButton buttonWithType:UIButtonTypeClose];[close addTarget:self action:@selector(cancel) forControlEvents:UIControlEventTouchUpInside];
 bar=[[UIProgressView alloc] initWithProgressViewStyle:UIProgressViewStyleBar];bar.hidden=YES;
 // The page loads behind this native screen; it is only revealed if pressing the buttons automatically fails.
 cover=[UIView new];cover.backgroundColor=[UIColor colorWithRed:0.05 green:0.09 blue:0.16 alpha:1];
 coverText=[UILabel new];coverText.text=@"Getting the game…";coverText.textColor=UIColor.whiteColor;coverText.font=[UIFont systemFontOfSize:20 weight:UIFontWeightSemibold];coverText.textAlignment=NSTextAlignmentCenter;coverText.numberOfLines=0;
 UIActivityIndicatorView *spin=[[UIActivityIndicatorView alloc] initWithActivityIndicatorStyle:UIActivityIndicatorViewStyleLarge];spin.color=UIColor.whiteColor;[spin startAnimating];
 coverBar=[[UIProgressView alloc] initWithProgressViewStyle:UIProgressViewStyleDefault];coverBar.hidden=YES;
 UIButton *coverClose=[UIButton buttonWithType:UIButtonTypeSystem];[coverClose setTitle:@"Cancel" forState:UIControlStateNormal];[coverClose addTarget:self action:@selector(cancel) forControlEvents:UIControlEventTouchUpInside];
 UIStackView *coverStack=[[UIStackView alloc] initWithArrangedSubviews:@[spin,coverText,coverBar,coverClose]];coverStack.axis=UILayoutConstraintAxisVertical;coverStack.alignment=UIStackViewAlignmentCenter;coverStack.spacing=18;coverStack.translatesAutoresizingMaskIntoConstraints=NO;[cover addSubview:coverStack];
 for(UIView *v in @[web,banner,close,bar,cover]){v.translatesAutoresizingMaskIntoConstraints=NO;[self.view addSubview:v];}
 UILayoutGuide *g=self.view.safeAreaLayoutGuide;
 [NSLayoutConstraint activateConstraints:@[
  [banner.topAnchor constraintEqualToAnchor:g.topAnchor],[banner.leadingAnchor constraintEqualToAnchor:g.leadingAnchor],[banner.trailingAnchor constraintEqualToAnchor:close.leadingAnchor constant:-8],[banner.heightAnchor constraintGreaterThanOrEqualToConstant:40],
  [close.centerYAnchor constraintEqualToAnchor:banner.centerYAnchor],[close.trailingAnchor constraintEqualToAnchor:g.trailingAnchor constant:-8],
  [bar.topAnchor constraintEqualToAnchor:banner.bottomAnchor],[bar.leadingAnchor constraintEqualToAnchor:self.view.leadingAnchor],[bar.trailingAnchor constraintEqualToAnchor:self.view.trailingAnchor],
  [web.topAnchor constraintEqualToAnchor:bar.bottomAnchor],[web.leadingAnchor constraintEqualToAnchor:self.view.leadingAnchor],[web.trailingAnchor constraintEqualToAnchor:self.view.trailingAnchor],[web.bottomAnchor constraintEqualToAnchor:self.view.bottomAnchor],
  [cover.topAnchor constraintEqualToAnchor:self.view.topAnchor],[cover.bottomAnchor constraintEqualToAnchor:self.view.bottomAnchor],[cover.leadingAnchor constraintEqualToAnchor:self.view.leadingAnchor],[cover.trailingAnchor constraintEqualToAnchor:self.view.trailingAnchor],
  [coverStack.centerXAnchor constraintEqualToAnchor:cover.centerXAnchor],[coverStack.centerYAnchor constraintEqualToAnchor:cover.centerYAnchor],[coverBar.widthAnchor constraintEqualToConstant:280]]];
 [WKContentRuleListStore.defaultStore compileContentRuleListForIdentifier:@"rdd-adblock" encodedContentRuleList:RDDAdBlockRules completionHandler:^(WKContentRuleList *list,NSError *e){
  if(list)[config.userContentController addContentRuleList:list];else fprintf(stderr,"ad block rules failed: %s\n",e.localizedDescription.UTF8String);
  [self->web loadRequest:[NSURLRequest requestWithURL:self.page]];}];
 // Give the automatic path 45 s to start the file; after that show the (ad-free) page so the user can tap.
 dispatch_after(dispatch_time(DISPATCH_TIME_NOW,45*NSEC_PER_SEC),dispatch_get_main_queue(),^{if(!self->active&&!self->destination)[self reveal];});
}
- (void)reveal{if(revealed)return;revealed=YES;fprintf(stderr,"download page revealed for a manual tap\n");[UIView animateWithDuration:0.25 animations:^{self->cover.alpha=0;} completion:^(BOOL f){self->cover.hidden=YES;}];}
- (void)cancel{[self dismissViewControllerAnimated:YES completion:nil];}
- (void)webView:(WKWebView*)w didFinishNavigation:(WKNavigation*)n{fprintf(stderr,"download page: %s\n",w.URL.absoluteString.UTF8String);
 // Pages can finish loading before their buttons exist (Cloudflare checks, late scripts), so retry a few times.
 for(int i=1;i<=4;i++)dispatch_after(dispatch_time(DISPATCH_TIME_NOW,(int64_t)(i*2.5*NSEC_PER_SEC)),dispatch_get_main_queue(),^{
  if(self->active||self->destination||self->revealed||w.isLoading)return;
  [w evaluateJavaScript:RDDPressDownloadJS completionHandler:^(id r,NSError *e){fprintf(stderr,"auto download press: %s\n",[[r description]?:e.localizedDescription UTF8String]);}];});
}
static BOOL RDDLooksLikeAPK(NSURLResponse *r){
 NSString *name=r.suggestedFilename.lowercaseString;NSString *type=r.MIMEType.lowercaseString;
 return [name hasSuffix:@".apk"]||[type isEqualToString:@"application/vnd.android.package-archive"];
}
- (void)webView:(WKWebView*)w decidePolicyForNavigationResponse:(WKNavigationResponse*)r decisionHandler:(void(^)(WKNavigationResponsePolicy))h{
 h(RDDLooksLikeAPK(r.response)||!r.canShowMIMEType?WKNavigationResponsePolicyDownload:WKNavigationResponsePolicyAllow);
}
- (void)webView:(WKWebView*)w decidePolicyForNavigationAction:(WKNavigationAction*)a decisionHandler:(void(^)(WKNavigationActionPolicy))h{
 h(a.shouldPerformDownload?WKNavigationActionPolicyDownload:WKNavigationActionPolicyAllow);
}
// Links that open new windows (target=_blank) load in place.
- (WKWebView*)webView:(WKWebView*)w createWebViewWithConfiguration:(WKWebViewConfiguration*)c forNavigationAction:(WKNavigationAction*)a windowFeatures:(WKWindowFeatures*)f{if(a.request.URL)[w loadRequest:a.request];return nil;}
- (void)webView:(WKWebView*)w navigationResponse:(WKNavigationResponse*)r didBecomeDownload:(WKDownload*)d{d.delegate=self;[self track:d];}
- (void)webView:(WKWebView*)w navigationAction:(WKNavigationAction*)a didBecomeDownload:(WKDownload*)d{d.delegate=self;[self track:d];}
// Mirrors often auto-start the file while the user also taps; keep only the first download.
- (void)track:(WKDownload*)d{if(active&&active!=d){fprintf(stderr,"duplicate download cancelled\n");[d cancel:nil];return;}active=d;fprintf(stderr,"download started: %s\n",d.originalRequest.URL.absoluteString.UTF8String);bar.hidden=NO;bar.observedProgress=d.progress;banner.text=@"Downloading the game…";coverBar.hidden=NO;coverBar.observedProgress=d.progress;coverText.text=@"Downloading the game (58 MB)…";}
- (void)download:(WKDownload*)d decideDestinationUsingResponse:(NSURLResponse*)r suggestedFilename:(NSString*)name completionHandler:(void(^)(NSURL*))h{
 if(d!=active){h(nil);return;}
 destination=[NSURL fileURLWithPath:[NSTemporaryDirectory() stringByAppendingPathComponent:[NSString stringWithFormat:@"%@.apk",NSUUID.UUID.UUIDString]]];h(destination);
}
- (void)downloadDidFinish:(WKDownload*)d{if(d!=active)return;active=nil;banner.text=@"Checking the download…";coverText.text=@"Checking the download…";NSURL *file=destination;
 dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{NSString *error=RDDImportAPK(file);[NSFileManager.defaultManager removeItemAtURL:file error:nil];
  dispatch_async(dispatch_get_main_queue(),^{
   if(!error){[self dismissViewControllerAnimated:YES completion:^{if(self.completion)self.completion(nil);}];}
   else{self->bar.hidden=YES;self->banner.text=[@"That download wasn't the right file. " stringByAppendingString:error];self->destination=nil;[self reveal];}
  });});
}
- (void)download:(WKDownload*)d didFailWithError:(NSError*)e resumeData:(NSData*)r{if(d!=active)return;active=nil;fprintf(stderr,"download failed: %s\n",e.localizedDescription.UTF8String);bar.hidden=YES;banner.text=[NSString stringWithFormat:@"Download failed: %@. Try again.",e.localizedDescription];destination=nil;[self reveal];}
@end
@protocol RDDImportDelegate<NSObject>
- (void)startGame;
@end
@interface RDDImportController:UIViewController<UIDocumentPickerDelegate>
@property(weak,nonatomic) id<RDDImportDelegate> delegate;
- (void)showError:(NSString*)message;
@end
@implementation RDDImportController{UILabel *status;UIButton *choose;UIActivityIndicatorView *spinner;}
- (void)viewDidLoad{[super viewDidLoad];
 self.view.backgroundColor=[UIColor colorWithRed:0.05 green:0.09 blue:0.16 alpha:1];
 UILabel *title=[UILabel new];title.text=@"9 Innings 2016";title.font=[UIFont systemFontOfSize:34 weight:UIFontWeightHeavy];title.textColor=UIColor.whiteColor;
 UILabel *body=[UILabel new];body.numberOfLines=0;body.textColor=[UIColor colorWithWhite:0.85 alpha:1];body.font=[UIFont systemFontOfSize:16];body.textAlignment=NSTextAlignmentCenter;
 body.text=@"Runs the original Android game, 9 Innings: Pro Baseball 2016 (v6.0.7).\nOne-time setup: get the game file (about 58 MB). After that it plays fully offline.";
 UIButton *download=[UIButton buttonWithType:UIButtonTypeSystem];UIButtonConfiguration *d=[UIButtonConfiguration filledButtonConfiguration];d.title=@"Download game";d.cornerStyle=UIButtonConfigurationCornerStyleCapsule;d.buttonSize=UIButtonConfigurationSizeLarge;download.configuration=d;
 [download addTarget:self action:@selector(download) forControlEvents:UIControlEventTouchUpInside];download.hidden=RDDDownloadSources().count==0;
 choose=[UIButton buttonWithType:UIButtonTypeSystem];UIButtonConfiguration *c=[UIButtonConfiguration plainButtonConfiguration];c.title=@"I already have the APK file";choose.configuration=c;
 [choose addTarget:self action:@selector(pick) forControlEvents:UIControlEventTouchUpInside];
 status=[UILabel new];status.numberOfLines=0;status.font=[UIFont systemFontOfSize:14];status.textColor=[UIColor colorWithRed:1 green:0.6 blue:0.55 alpha:1];status.textAlignment=NSTextAlignmentCenter;
 UILabel *hint=[UILabel new];hint.numberOfLines=0;hint.font=[UIFont systemFontOfSize:13];hint.textColor=[UIColor colorWithWhite:0.6 alpha:1];hint.textAlignment=NSTextAlignmentCenter;
 hint.text=@"Have the .apk already? Choose it above, share/AirDrop it to this app, or drop it into the app's folder from a computer (Finder → iPhone → Files).";
 spinner=[[UIActivityIndicatorView alloc] initWithActivityIndicatorStyle:UIActivityIndicatorViewStyleLarge];spinner.color=UIColor.whiteColor;spinner.hidesWhenStopped=YES;
 UIStackView *stack=[[UIStackView alloc] initWithArrangedSubviews:@[title,body,download,choose,spinner,status,hint]];stack.axis=UILayoutConstraintAxisVertical;stack.alignment=UIStackViewAlignmentCenter;stack.spacing=16;stack.translatesAutoresizingMaskIntoConstraints=NO;
 [self.view addSubview:stack];UILayoutGuide *g=self.view.safeAreaLayoutGuide;
 [NSLayoutConstraint activateConstraints:@[[stack.centerXAnchor constraintEqualToAnchor:g.centerXAnchor],[stack.centerYAnchor constraintEqualToAnchor:g.centerYAnchor],[stack.widthAnchor constraintLessThanOrEqualToConstant:560],[stack.leadingAnchor constraintGreaterThanOrEqualToAnchor:g.leadingAnchor constant:24],[stack.trailingAnchor constraintLessThanOrEqualToAnchor:g.trailingAnchor constant:-24]]];
}
- (BOOL)prefersStatusBarHidden{return YES;}
- (UIInterfaceOrientationMask)supportedInterfaceOrientations{return UIInterfaceOrientationMaskLandscape;}
- (void)pick{UTType *apk=[UTType typeWithFilenameExtension:@"apk"]?:UTTypeData;
 UIDocumentPickerViewController *picker=[[UIDocumentPickerViewController alloc] initForOpeningContentTypes:@[apk,UTTypeZIP,UTTypeData] asCopy:YES];picker.delegate=self;[self presentViewController:picker animated:YES completion:nil];}
- (void)viewDidAppear:(BOOL)animated{[super viewDidAppear:animated];static BOOL once;if(getenv("RDD_AUTOTEST_DOWNLOAD")&&!once){once=YES;[self download];}}
- (void)download{NSDictionary *source=RDDDownloadSources().firstObject;if(!source)return;
 RDDDownloadController *web=[RDDDownloadController new];web.page=[NSURL URLWithString:source[@"page"]];web.modalPresentationStyle=UIModalPresentationFullScreen;
 __weak typeof(self) weakSelf=self;web.completion=^(NSString *error){if(!error)[weakSelf.delegate startGame];};
 [self presentViewController:web animated:YES completion:nil];}
- (void)documentPicker:(UIDocumentPickerViewController*)controller didPickDocumentsAtURLs:(NSArray<NSURL*>*)urls{
 NSURL *url=urls.firstObject;if(!url)return;status.text=@"Checking the APK…";status.textColor=[UIColor colorWithWhite:0.85 alpha:1];choose.enabled=NO;[spinner startAnimating];
 dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{NSString *error=RDDImportAPK(url);
  dispatch_async(dispatch_get_main_queue(),^{[self->spinner stopAnimating];self->choose.enabled=YES;if(error)[self showError:error];else[self.delegate startGame];});});
}
- (void)showError:(NSString*)message{status.textColor=[UIColor colorWithRed:1 green:0.6 blue:0.55 alpha:1];status.text=message;}
@end

@interface RDDGameDelegate:UIResponder<UIApplicationDelegate,RDDImportDelegate>
@property(strong,nonatomic) UIWindow *window;
@end
@implementation RDDGameDelegate
- (BOOL)application:(UIApplication*)application didFinishLaunchingWithOptions:(NSDictionary*)options{
 NSString *documents=NSSearchPathForDirectoriesInDomains(NSDocumentDirectory,NSUserDomainMask,YES).firstObject;chdir(documents.UTF8String);
 NSFileManager *fm=NSFileManager.defaultManager;for(int i=3;i>=1;i--){NSString *old=[documents stringByAppendingPathComponent:i==1?@"engine.log":[NSString stringWithFormat:@"engine.previous-%d.log",i-1]];NSString *new=[documents stringByAppendingPathComponent:[NSString stringWithFormat:@"engine.previous-%d.log",i]];[fm removeItemAtPath:new error:nil];if([fm fileExistsAtPath:old])[fm moveItemAtPath:old toPath:new error:nil];}
 freopen([[documents stringByAppendingPathComponent:@"engine.log"] UTF8String],"w",stdout);setvbuf(stdout,NULL,_IOLBF,0);dup2(fileno(stdout),STDERR_FILENO);
 fprintf(stderr,"engine session: %s build=%s %s offline-network-only=1\n",NSDate.date.description.UTF8String,__DATE__,__TIME__);
 for(int i=0;i<5;i++){int signals[]={SIGSEGV,SIGBUS,SIGABRT,SIGILL,SIGFPE};signal(signals[i],rdd_crash_signal);}
 self.window=[[UIWindow alloc] initWithFrame:UIScreen.mainScreen.bounds];application.idleTimerDisabled=YES;
 RDDAdoptBundledOrSharedAPK();
 if(RDDInstalledAPKIsValid()){[self startGame];}else{RDDImportController *import=[RDDImportController new];import.delegate=self;self.window.rootViewController=import;}
 [self.window makeKeyAndVisible];return YES;
}
- (void)startGame{if(gameStarted)return;gameStarted=YES;self.window.rootViewController=[RDDGameController new];RDDStartTouchScript();dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0),^{@autoreleasepool{RDDSaveAPKIcon();rdd_run_game();}});}
// "Open in 9 Innings", AirDrop and the share sheet deliver the APK here.
- (BOOL)application:(UIApplication*)app openURL:(NSURL*)url options:(NSDictionary*)options{
 if(gameStarted)return NO;NSString *error=RDDImportAPK(url);
 if(!error){[self startGame];return YES;}
 if([self.window.rootViewController isKindOfClass:RDDImportController.class])[(RDDImportController*)self.window.rootViewController showError:error];
 return NO;
}
- (void)applicationDidReceiveMemoryWarning:(UIApplication*)application{fprintf(stderr,"WARNING iOS memory pressure\n");}
- (void)applicationDidEnterBackground:(UIApplication*)application{fprintf(stderr,"lifecycle: background\n");}
- (void)applicationWillEnterForeground:(UIApplication*)application{fprintf(stderr,"lifecycle: foreground\n");}
@end
int rdd_application_main(void){@autoreleasepool{char *args[]={"NineInnings",NULL};return UIApplicationMain(1,args,nil,NSStringFromClass(RDDGameDelegate.class));}}
int rdd_bundle_path(char *out,size_t capacity){const char *path=NSBundle.mainBundle.bundlePath.UTF8String;size_t n=strlen(path);if(n+1>capacity)return -1;memcpy(out,path,n+1);return (int)n;}

static UIView *progressOverlay;
void rdd_progress(const unsigned char *message,size_t length,int show){@autoreleasepool{NSString *text=show?[[NSString alloc] initWithBytes:message length:length encoding:NSUTF8StringEncoding]:nil;dispatch_async(dispatch_get_main_queue(),^{[progressOverlay removeFromSuperview];progressOverlay=nil;if(!show||!gameView)return;UIView *overlay=[[UIView alloc] initWithFrame:gameView.bounds];overlay.autoresizingMask=UIViewAutoresizingFlexibleWidth|UIViewAutoresizingFlexibleHeight;overlay.backgroundColor=[UIColor colorWithWhite:0 alpha:0.4];UIView *panel=[[UIView alloc] initWithFrame:CGRectMake((overlay.bounds.size.width-300)/2,(overlay.bounds.size.height-76)/2,300,76)];panel.autoresizingMask=UIViewAutoresizingFlexibleLeftMargin|UIViewAutoresizingFlexibleRightMargin|UIViewAutoresizingFlexibleTopMargin|UIViewAutoresizingFlexibleBottomMargin;panel.backgroundColor=UIColor.whiteColor;panel.layer.cornerRadius=8;UIActivityIndicatorView *spinner=[[UIActivityIndicatorView alloc] initWithActivityIndicatorStyle:UIActivityIndicatorViewStyleMedium];spinner.center=CGPointMake(30,38);[spinner startAnimating];[panel addSubview:spinner];UILabel *label=[[UILabel alloc] initWithFrame:CGRectMake(58,8,232,60)];label.text=text;label.numberOfLines=2;label.textColor=UIColor.blackColor;label.font=[UIFont systemFontOfSize:16];[panel addSubview:label];[overlay addSubview:panel];[gameView.superview addSubview:overlay];progressOverlay=overlay;});}}

int rdd_text_metrics(const char *fontpath,double size,int32_t *metrics){@autoreleasepool{CTFontRef font=rdd_copy_font(fontpath,size);if(!font)return -1;CGRect bounds=CTFontGetBoundingBox(font);metrics[0]=(int32_t)floor(-CGRectGetMaxY(bounds));metrics[1]=(int32_t)round(-CTFontGetAscent(font));metrics[2]=(int32_t)round(CTFontGetDescent(font));CFRelease(font);return 0;}}

int rdd_text_rect(const char *fontpath,const uint16_t *text,size_t length,double size,uint32_t width,uint32_t height,int align,int underline,const int32_t *lines,size_t count,uint32_t *pixels){@autoreleasepool{if(!width||!height)return 0;CTFontRef font=rdd_copy_font(fontpath,size);if(!font)return -1;NSString *string=[[NSString alloc] initWithCharacters:text length:length];NSDictionary *attrs=@{(__bridge NSString*)kCTFontAttributeName:(__bridge id)font,(__bridge NSString*)kCTForegroundColorFromContextAttributeName:@YES};uint8_t *rgba=calloc(width*height,4);if(!rgba){CFRelease(font);return -1;}CGColorSpaceRef space=CGColorSpaceCreateDeviceRGB();CGContextRef ctx=CGBitmapContextCreate(rgba,width,height,8,width*4,space,kCGBitmapByteOrder32Big|kCGImageAlphaPremultipliedLast);CGColorSpaceRelease(space);if(!ctx){free(rgba);CFRelease(font);return -1;}CGContextSetRGBFillColor(ctx,1,1,1,1);for(size_t i=0;i<count;i++){const int32_t *line=lines+i*4;NSAttributedString *attributed=[[NSAttributedString alloc] initWithString:[string substringWithRange:NSMakeRange(line[0],line[1]-line[0])] attributes:attrs];CTLineRef run=CTLineCreateWithAttributedString((__bridge CFAttributedStringRef)attributed);double w=CTLineGetTypographicBounds(run,NULL,NULL,NULL);double x=line[2]-(align==1?w/2:align==2?w:0),y=height-line[3];CGContextSetTextPosition(ctx,x,y);CTLineDraw(run,ctx);if(underline){CGContextSetRGBStrokeColor(ctx,1,1,1,1);CGContextSetLineWidth(ctx,CTFontGetUnderlineThickness(font));CGFloat uy=y+CTFontGetUnderlinePosition(font);CGContextMoveToPoint(ctx,x,uy);CGContextAddLineToPoint(ctx,x+w,uy);CGContextStrokePath(ctx);}CFRelease(run);}for(uint32_t y=0;y<height;y++)for(uint32_t x=0;x<width;x++){uint8_t alpha=rgba[(y*width+x)*4+3];pixels[y*width+x]=alpha?((uint32_t)alpha<<24)|0x00ffffff:0;}CGContextRelease(ctx);free(rgba);CFRelease(font);return 0;}}

#include <CoreFoundation/CFStringEncodingExt.h>
int rdd_ms949_utf8(const uint8_t *input,size_t length,uint8_t *output,size_t capacity){@autoreleasepool{NSStringEncoding encoding=CFStringConvertEncodingToNSStringEncoding(kCFStringEncodingDOSKorean);NSString *string=[[NSString alloc] initWithBytes:input length:length encoding:encoding];if(!string)return -1;NSData *data=[string dataUsingEncoding:NSUTF8StringEncoding];if(data.length>capacity)return -2;memcpy(output,data.bytes,data.length);return (int)data.length;}}
